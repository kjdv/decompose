extern crate nix;
extern crate tokio;

use super::*;

use log;
use std::collections::HashMap;

use graph::{Graph, NodeHandle};
use nix::sys::signal as nix_signal;
use std::process::Stdio;
use tokio::process::Command;
use tokio::signal;
use tokio::signal::unix::{signal, SignalKind};
use tokio::sync::mpsc;

type TokResult<T> = std::result::Result<T, tokio::io::Error>;
type Result<T> = std::result::Result<T, Box<dyn Error>>;

pub struct Executor {
    dependency_graph: Graph,
    running: HashMap<NodeHandle, Option<Process>>,
    terminate_timeout: std::time::Duration,
}

impl Executor {
    pub fn from_config(cfg: &config::System) -> Result<Executor> {
        let graph = Graph::from_config(&cfg)?;

        Ok(Executor {
            dependency_graph: graph,
            running: HashMap::new(),
            terminate_timeout: std::time::Duration::from_secs_f64(cfg.terminate_timeout),
        })
    }

    pub async fn start(&mut self) -> Result<()> {
        let (tx, mut rx) = mpsc::channel(100);

        self.dependency_graph.roots().for_each(|h| {
            let p = self.dependency_graph.node(h).clone();
            let tx = tx.clone();

            log::info!("starting program {}", p.name);
            tokio::spawn(start_program(h, p, tx));
        });

        while let Some(msg) = rx.recv().await {
            match msg {
                Ok((h, p)) => {
                    self.running.insert(h, Some(p));

                    self.dependency_graph
                        .expand(h, |i| self.running.contains_key(&i))
                        .for_each(|n| {
                            let p = self.dependency_graph.node(n).clone();
                            let tx = tx.clone();

                            log::info!("starting program {}", p.name);
                            tokio::spawn(start_program(n, p, tx));
                        });
                }
                Err(e) => {
                    return Err(e.into());
                }
            };

            if self.running.len() == self.dependency_graph.len() {
                break;
            }
        }

        Ok(())
    }

    pub async fn wait(&self) -> Result<()> {
        let r = tokio::select! {
            x = self.wait_for_signal(SignalKind::interrupt()) => x,
            x = self.wait_for_signal(SignalKind::terminate()) => x,
        };
        r.map_err(|e| e.into())
    }

    pub async fn stop(&mut self) {
        let (tx, mut rx) = mpsc::channel(100);

        let leaves: Vec<_> = self.dependency_graph.leaves().collect();
        leaves.iter().for_each(|h| {
            let op = self.running.get_mut(&h).expect("no process for node");
            if let Some(p) = op.take() {
                log::info!("stopping program {}", self.dependency_graph.node(*h).name);

                tokio::spawn(stop_program(*h, p, self.terminate_timeout, tx.clone()));
            }
        });

        while let Some(h) = rx.recv().await {
            let expanded: Vec<_> = self
                .dependency_graph
                .expand_back(h, |i| !self.running.contains_key(&i))
                .collect();

            expanded.iter().for_each(|h| {
                let op = self.running.get_mut(&h).expect("no process for node");
                if let Some(p) = op.take() {
                    log::info!("stopping program {}", self.dependency_graph.node(*h).name);

                    tokio::spawn(stop_program(*h, p, self.terminate_timeout, tx.clone()));
                }
            });

            self.running.remove(&h);

            if self.running.is_empty() {
                break;
            }
        }

        assert!(self.running.is_empty());
    }

    async fn wait_for_signal(&self, kind: SignalKind) -> TokResult<()> {
        let mut sig = signal(kind)?;
        sig.recv().await;
        log::info!("received signal {:?}", kind);
        Ok(())
    }
}

impl Drop for Executor {
    fn drop(&mut self) {
        // optimize: don't bother constructing a runtime if everything is stopped already
        if !self.running.is_empty() {
            run(self.stop());
        }
    }
}

#[derive(Debug)]
struct Process {
    proc: Box<tokio::process::Child>,
    info: ProcessInfo,
}

#[derive(Debug, Clone)]
struct ProcessInfo {
    name: String,
    pid: u32,
}

impl Process {
    fn new(proc: tokio::process::Child, name: String) -> Process {
        let pid = proc.id();
        Process {
            proc: Box::new(proc),
            info: ProcessInfo { name, pid },
        }
    }

    fn is_alive(&self) -> bool {
        is_alive(self.info.pid)
    }
}

impl std::fmt::Display for Process {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        self.info.fmt(f)
    }
}

impl std::fmt::Display for ProcessInfo {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        write!(f, "{}:{}", self.name, self.pid)
    }
}

pub fn run<F: futures::future::Future>(f: F) -> F::Output {
    let mut rt = tokio::runtime::Builder::new()
        .basic_scheduler()
        .enable_all()
        .build()
        .expect("runtime");

    let result = rt.block_on(f);
    rt.shutdown_timeout(std::time::Duration::from_secs(1));
    result
}

async fn start_program(
    h: NodeHandle,
    prog: config::Program,
    mut completed: mpsc::Sender<TokResult<(NodeHandle, Process)>>,
) {
    let msg = do_start_program(prog).await.map(|p| (h, p));
    completed.send(msg).await.expect("channel error");
}

async fn do_start_program(prog: config::Program) -> TokResult<Process> {
    let child = Command::new(&prog.argv[0])
        .args(&prog.argv.as_slice()[1..])
        .envs(&prog.env)
        .current_dir(prog.cwd)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true)
        .spawn()?;
    let proc = Process::new(child, prog.name);

    log::info!("{} started", proc.info);

    tokio::time::delay_for(tokio::time::Duration::from_secs(1)).await;

    log::info!("{} ready", proc.info);

    Ok(proc)
}

async fn stop_program(
    h: NodeHandle,
    proc: Process,
    timeout: std::time::Duration,
    mut completed: mpsc::Sender<NodeHandle>,
) {
    do_stop(proc, timeout).await;
    completed.send(h).await.expect("channel error");
}

async fn do_stop(proc: Process, timeout: std::time::Duration) {
    let info = proc.info.clone();
    match terminate_wait(proc, timeout).await {
        Ok(status) => log::info!("{} stopped with status {}", info, status),
        Err(e) => log::warn!("{} killed, failed to terminate: {}", info, e),
    };

    assert!(!is_alive(info.pid), "terminate_wait failed to kill on drop");
}

fn is_alive(pid: u32) -> bool {
    let pid = nix::unistd::Pid::from_raw(pid as i32);
    nix_signal::kill(pid, None).is_ok()
}

fn terminate(pid: u32) -> Result<()> {
    let pid = nix::unistd::Pid::from_raw(pid as i32);
    let sig = nix_signal::Signal::SIGTERM;

    nix_signal::kill(pid, sig).map_err(|e| e.into())
}

async fn terminate_wait(
    proc: Process,
    timeout: std::time::Duration,
) -> Result<std::process::ExitStatus> {
    let pid = proc.info.pid;
    terminate(pid)?;

    tokio::select! {
        x = proc.proc.wait_with_output() => {
            match x {
                Ok(o) => Ok(o.status),
                Err(e) => Err(e.into()),
            }
        }
        _ = tokio::time::delay_for(timeout) => Err(string_error::static_err("timeout")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    extern crate regex;

    #[tokio::test]
    async fn is_alive_and_stop() {
        let proc = Process::new(
            Command::new("/bin/cat")
                .kill_on_drop(true)
                .spawn()
                .expect("cat"),
            "cat".to_string(),
        );
        let pid = proc.info.pid;

        assert!(proc.is_alive());

        let timeout = std::time::Duration::from_millis(1);
        do_stop(proc, timeout).await;

        assert!(!is_alive(pid));
    }

    #[tokio::test]
    async fn format_process() {
        let re = regex::Regex::new("catname:[0-9]+").expect("re");

        let proc = Process::new(
            Command::new("/bin/cat")
                .kill_on_drop(true)
                .spawn()
                .expect("cat"),
            "catname".to_string(),
        );

        let fmt = format!("{}", proc.info);
        assert!(re.is_match(fmt.as_str()));

        let fmt = format!("{}", proc);
        assert!(re.is_match(fmt.as_str()));
    }
}
