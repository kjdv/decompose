extern crate nix;
extern crate tokio;

use super::*;

use log;
use std::collections::HashMap;

use graph::{Graph, NodeHandle};
use nix::sys::signal as nix_signal;
use std::process::Stdio;
use tokio::process::Command;
use tokio::signal::unix::SignalKind;
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
        log::info!("starting execution");

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
                    log::warn!("failed to start: {}", e);
                    return Err(e.into());
                }
            };

            if self.running.len() == self.dependency_graph.len() {
                break;
            }
        }

        Ok(())
    }

    pub async fn run(&mut self) -> Result<()> {
        loop {
            let r = tokio::select! {
                _ = wait_for_signal(SignalKind::child()) => {
                    self.check_alive();

                    if self.running.is_empty() {
                        log::info!("no running processes left");
                        Ok(false)
                    } else {
                        Ok(true)
                    }
                },
                x = wait_for_signal(SignalKind::interrupt()) => x.map(|_| false),
                x = wait_for_signal(SignalKind::terminate()) => x.map(|_| false),
            };
            match r {
                Ok(true) => (),
                Ok(false) => {
                    return Ok(());
                }
                Err(e) => return Err(e.into()),
            }
        }
    }

    pub async fn stop(&mut self) {
        let (tx, mut rx) = mpsc::channel(100);

        let leaves: Vec<_> = self.dependency_graph.leaves().collect();
        leaves.iter().for_each(|h| {
            self.send_stop(*h, tx.clone());
        });

        while let Some(h) = rx.recv().await {
            let expanded: Vec<_> = self
                .dependency_graph
                .expand_back(h, |i| !self.running.contains_key(&i))
                .collect();

            expanded.iter().for_each(|h| {
                self.send_stop(*h, tx.clone());
            });

            self.running.remove(&h);

            if self.running.is_empty() {
                break;
            }
        }

        assert!(self.running.is_empty());
        log::info!("stopping execution");
    }

    fn send_stop(&mut self, h: NodeHandle, tx: mpsc::Sender<NodeHandle>) {
        match self.running.get_mut(&h) {
            Some(op) => {
                if let Some(p) = op.take() {
                    log::info!("stopping program {}", self.dependency_graph.node(h).name);

                    tokio::spawn(stop_program(h, p, self.terminate_timeout, tx));
                }
            }
            None => {
                log::debug!("process for handle already stopped");
                tokio::spawn(dummy_stop(h, tx));
            }
        }
    }

    fn check_alive(&mut self) {
        let alive = |p: &Process| {
            if p.is_alive() {
                log::debug!("{} still alive", p);
                true
            } else {
                log::info!("{} stopped", p);
                false
            }
        };
        self.running.retain(|_, v| v.as_ref().map_or(false, alive));
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
    use config::ReadySignal;
    use tokio::io::{Error, ErrorKind};

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

    let rs = match prog.ready {
        ReadySignal::Nothing => readysignals::nothing().await?,
        ReadySignal::Manual => readysignals::manual(proc.info.name.as_str()).await?,
        ReadySignal::Timer(s) => {
            let dur = std::time::Duration::from_secs_f64(s);
            log::debug!("waiting {}s for {}", s, proc);
            readysignals::timer(dur).await?
        }
        ReadySignal::Port(port) => {
            log::debug!("waiting for port {} for {}", port, proc);
            readysignals::port(port).await?
        }
        _ => readysignals::nothing().await?,
    };

    match rs {
        true => {
            log::info!("{} ready", proc.info);
            Ok(proc)
        }
        false => {
            let msg = format!("{} not ready", proc.info);
            log::error!("{}", msg);
            Err(Error::new(ErrorKind::Other, msg))
        }
    }
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
        Ok(status) => {
            log::debug!("{} exit status {}", info, status);
            log::info!("{} terminated", info);
        }
        Err(e) => {
            log::debug!("{} failed to terminate: {}", info, e);
            log::warn!("{} killed", info);
        }
    };
}

async fn dummy_stop(h: NodeHandle, mut completed: mpsc::Sender<NodeHandle>) {
    completed.send(h).await.expect("channel error");
}

fn is_alive(pid: u32) -> bool {
    use nix::sys::wait;

    let pid = nix::unistd::Pid::from_raw(pid as i32);
    match wait::waitpid(pid, Some(wait::WaitPidFlag::WNOHANG)) {
        Ok(wait::WaitStatus::StillAlive) => true,
        _ => false,
    }
}

fn terminate(pid: u32) -> Result<()> {
    let pid = nix::unistd::Pid::from_raw(pid as i32);
    let sig = nix_signal::Signal::SIGTERM;

    nix_signal::kill(pid, sig).map_err(|e| e.into())
}

async fn wait_for_signal(kind: SignalKind) -> TokResult<()> {
    use tokio::signal::unix::signal;

    let mut sig = signal(kind)?;
    sig.recv().await;
    log::info!("received signal {:?}", kind);
    Ok(())
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
        },
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
