extern crate nix;
extern crate tokio;

use super::config;
use super::output;
use super::readysignals;
use super::tokio_utils;

use std::collections::HashMap;

use super::graph::{Graph, NodeHandle};
use nix::sys::signal as nix_signal;
use std::time::Duration;
use tokio::process::Command;
use tokio::signal::unix::SignalKind;
use tokio::sync::mpsc;

type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;

pub struct Executor {
    dependency_graph: Graph,
    running: HashMap<NodeHandle, Option<Process>>,
    start_timeout: Option<Duration>,
    terminate_timeout: Duration,
}

impl Executor {
    pub fn from_config(cfg: &config::System) -> Result<Executor> {
        let graph = Graph::from_config(&cfg)?;

        Ok(Executor {
            dependency_graph: graph,
            running: HashMap::new(),
            start_timeout: cfg.start_timeout.map(Duration::from_secs_f64),
            terminate_timeout: Duration::from_secs_f64(cfg.terminate_timeout),
        })
    }

    pub async fn start(
        &mut self,
        mut output_factory: Box<dyn output::OutputFactory>,
    ) -> Result<()> {
        log::info!("starting execution");

        let (tx, mut rx) = mpsc::channel(100);

        self.dependency_graph.roots().for_each(|h| {
            let p = self.dependency_graph.node(h).clone();
            let tx = tx.clone();

            log::info!("starting program {}", p.name);

            let (stdout, stderr) = (output_factory.stdout(&p), output_factory.stderr(&p));
            tokio::spawn(start_program(h, p, stdout, stderr, self.start_timeout, tx));
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
                            let (stdout, stderr) =
                                (output_factory.stdout(&p), output_factory.stderr(&p));
                            tokio::spawn(start_program(
                                n,
                                p,
                                stdout,
                                stderr,
                                self.start_timeout,
                                tx,
                            ));
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
        let res = self.do_run().await;
        self.stop().await;
        res
    }

    async fn do_run(&mut self) -> Result<()> {
        loop {
            let r = tokio::select! {
                _ = wait_for_signal(SignalKind::child()) => {
                    let teardown = self.check_alive();

                    if teardown {
                        log::info!("critical program stoppend, tearing down system");
                        self.stop().await;
                    }

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

    async fn stop(&mut self) {
        let (tx, mut rx) = mpsc::channel(100);

        let leaves: Vec<_> = self.dependency_graph.leaves().collect();
        leaves.iter().for_each(|h| {
            self.send_stop(*h, tx.clone());
        });

        while let Some(h) = rx.recv().await {
            self.running.remove(&h);

            let expanded: Vec<_> = self
                .dependency_graph
                .expand_back(h, |i| !self.running.contains_key(&i))
                .collect();

            expanded.iter().for_each(|h| {
                self.send_stop(*h, tx.clone());
            });

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
                    log::debug!("stopping program {}", p);

                    tokio::spawn(stop_program(h, p, self.terminate_timeout, tx));
                }
            }
            None => {
                log::debug!("process for handle already stopped");
                tokio::spawn(dummy_stop(h, tx));
            }
        }
    }

    fn check_alive(&mut self) -> bool {
        let alive = |p: &Process| {
            if p.is_alive() {
                log::debug!("{} still alive", p);
                true
            } else {
                log::info!("{} stopped", p);
                false
            }
        };
        let teardown_check = |p: &Process| !alive(p) && p.critical;

        let teardown = self
            .running
            .iter()
            .any(|(_, v)| v.as_ref().map_or(false, teardown_check));
        self.running.retain(|_, v| v.as_ref().map_or(false, alive));
        teardown
    }
}

#[derive(Debug)]
struct Process {
    proc: Option<Box<tokio::process::Child>>,
    info: ProcessInfo,
    critical: bool,
}

#[derive(Debug, Clone)]
struct ProcessInfo {
    pub name: String,
    pub pid: u32,
}

impl Process {
    fn new(proc: Option<tokio::process::Child>, info: ProcessInfo, critical: bool) -> Process {
        Process {
            proc: proc.map(Box::new),
            info,
            critical,
        }
    }

    #[allow(dead_code)]
    fn new_with_name(proc: tokio::process::Child, name: String, critical: bool) -> Process {
        let pid = proc.id();
        Process {
            proc: Some(Box::new(proc)),
            info: ProcessInfo { name, pid },
            critical,
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

async fn start_program(
    h: NodeHandle,
    prog: config::Program,
    stdout: output::Sender,
    stderr: output::Sender,
    timeout: Option<Duration>,
    mut completed: mpsc::Sender<tokio_utils::Result<(NodeHandle, Process)>>,
) {
    let msg = match timeout {
        Some(t) => tokio_utils::with_timeout(do_start_program(prog, stdout, stderr), t).await,
        None => do_start_program(prog, stdout, stderr).await,
    }
    .map(|p| (h, p));
    completed.send(msg).await.expect("channel error");
}

async fn do_start_program(
    prog: config::Program,
    stdout: output::Sender,
    stderr: output::Sender,
) -> tokio_utils::Result<Process> {
    use config::ReadySignal;

    let (mut child, info) = create_child_process(&prog)?;

    let monitor_out = stdout.subscribe();
    let monitor_err = stderr.subscribe();

    tokio::spawn(output::produce(stdout, child.stdout.take()));
    tokio::spawn(output::produce(stderr, child.stderr.take()));

    let mut child = Some(child);

    log::info!("{} started", info);

    let rs = match prog.ready {
        ReadySignal::Nothing => readysignals::nothing().await?,
        ReadySignal::Manual => readysignals::manual(info.name.as_str()).await?,
        ReadySignal::Timer(s) => {
            let dur = Duration::from_secs_f64(s);
            log::debug!("waiting {}s for {}", s, info);
            readysignals::timer(dur).await?
        }
        ReadySignal::Port(port) => {
            log::debug!("waiting for port {} for {}", port, info);
            readysignals::port(port).await?
        }
        ReadySignal::Completed => readysignals::completed(child.take().unwrap()).await?,
        ReadySignal::Stdout(re) => readysignals::output(monitor_out, re.as_str()).await?,
        ReadySignal::Stderr(re) => readysignals::output(monitor_err, re.as_str()).await?,
        ReadySignal::Healthcheck(endpoint) => {
            readysignals::healthcheck(
                endpoint.host.as_str(),
                endpoint.port,
                endpoint.path.as_str(),
            )
            .await?
        }
    };

    match rs {
        true => {
            log::info!("{} ready", info);
            Ok(Process::new(child, info, prog.critical))
        }
        false => {
            let msg = format!("{} not ready", info);
            log::error!("{}", msg);
            Err(tokio_utils::make_err(msg))
        }
    }
}

fn create_child_process(
    prog: &config::Program,
) -> tokio_utils::Result<(tokio::process::Child, ProcessInfo)> {
    use std::str::FromStr;

    let executable = std::fs::canonicalize(&prog.exec)
        .or_else(|_| std::path::PathBuf::from_str(&prog.exec))
        .map_err(tokio_utils::make_err)?;
    let current_dir = std::fs::canonicalize(prog.cwd.clone())?;
    log::debug!(
        "executable {:?}, current dir will be {:?}",
        executable,
        current_dir
    );

    let child = Command::new(executable)
        .args(&prog.args)
        .envs(&prog.env)
        .current_dir(current_dir)
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .kill_on_drop(true)
        .spawn()?;
    let info = ProcessInfo {
        name: prog.name.clone(),
        pid: child.id(),
    };

    Ok((child, info))
}

async fn stop_program(
    h: NodeHandle,
    proc: Process,
    timeout: Duration,
    mut completed: mpsc::Sender<NodeHandle>,
) {
    do_stop(proc, timeout).await;
    completed.send(h).await.expect("channel error");
}

async fn do_stop(proc: Process, timeout: Duration) {
    let info = proc.info.clone();
    match terminate_wait(proc, timeout).await {
        Ok(Some(status)) => {
            log::debug!("{} exit status {}", info, status);
            log::info!("{} terminated", info);
        }
        Ok(None) => {
            log::debug!("{} nothing to terminate", info);
        }
        Err(e) => {
            log::debug!("{} failed to terminate: {}", info, e);
            log::warn!("{} killed", info);
        }
    };
}

async fn dummy_stop(h: NodeHandle, mut completed: mpsc::Sender<NodeHandle>) {
    if let Err(e) = completed.send(h).await {
        log::warn!("{}", e);
    }
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

async fn wait_for_signal(kind: SignalKind) -> tokio_utils::Result<()> {
    use tokio::signal::unix::signal;

    let mut sig = signal(kind)?;
    sig.recv().await;
    log::info!("received signal {:?}", kind);
    Ok(())
}

async fn terminate_wait(
    mut proc: Process,
    timeout: Duration,
) -> Result<Option<std::process::ExitStatus>> {
    if let Some(p) = proc.proc.take() {
        let pid = proc.info.pid;
        terminate(pid)?;

        tokio_utils::with_timeout(p.wait_with_output(), timeout)
            .await
            .map(|ok| Some(ok.status))
            .map_err(|e| e.into())
    } else {
        Ok(None)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    extern crate regex;

    #[tokio::test]
    async fn is_alive_and_stop() {
        let proc = Process::new_with_name(
            Command::new("/bin/cat")
                .kill_on_drop(true)
                .spawn()
                .expect("cat"),
            "cat".to_string(),
            false,
        );
        let pid = proc.info.pid;

        assert!(proc.is_alive());

        let timeout = Duration::from_millis(1);
        do_stop(proc, timeout).await;

        assert!(!is_alive(pid));
    }

    #[tokio::test]
    async fn format_process() {
        let re = regex::Regex::new("catname:[0-9]+").expect("re");

        let proc = Process::new_with_name(
            Command::new("/bin/cat")
                .kill_on_drop(true)
                .spawn()
                .expect("cat"),
            "catname".to_string(),
            false,
        );

        let fmt = format!("{}", proc.info);
        assert!(re.is_match(fmt.as_str()));

        let fmt = format!("{}", proc);
        assert!(re.is_match(fmt.as_str()));
    }
}
