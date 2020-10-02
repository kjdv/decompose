extern crate tokio;

use super::config;
use super::graph::NodeHandle;
use super::output;
use super::readysignals;
use super::tokio_utils;
use std::collections::HashMap;
use std::time::Duration;
use tokio::process;
pub use tokio::sync::mpsc;

#[derive(Debug, Clone)]
pub enum Command {
    Start((NodeHandle, config::Program)),
    Stop(NodeHandle),
}

#[derive(Debug)]
pub enum Event {
    Started(NodeHandle),
    Stopped(NodeHandle),
    Shutdown,
    Err(tokio::io::Error),
}

pub struct ProcessManager {
    rx: mpsc::Receiver<Command>,
    tx: mpsc::Sender<Event>,
    output_factory: Box<dyn output::OutputFactory>,
    start_timeout: Option<Duration>,
    terminate_timeout: Duration,
    procs: HashMap<NodeHandle, Process>,
}

impl ProcessManager {
    pub fn new(
        rx: mpsc::Receiver<Command>,
        tx: mpsc::Sender<Event>,
        sys: &config::System,
        output_factory: Box<dyn output::OutputFactory>,
    ) -> ProcessManager {
        ProcessManager {
            rx,
            tx,
            output_factory,
            start_timeout: sys.start_timeout.map(Duration::from_secs_f64),
            terminate_timeout: Duration::from_secs_f64(sys.terminate_timeout),
            procs: HashMap::new(),
        }
    }

    pub async fn run(mut self) -> std::result::Result<(), Box<dyn std::error::Error>> {
        loop {
            let c = tokio::select! {
                _ = tokio_utils::wait_for_signal(tokio_utils::SignalKind::child()) => {
                    log::debug!("received SIGCHILD");
                    self.check_alive().await;
                    true
                },
                _ = tokio_utils::wait_for_signal(tokio_utils::SignalKind::interrupt()) => {
                    log::debug!("received SIGINT");
                    self.send(Event::Shutdown).await;
                    true
                },
                _ = tokio_utils::wait_for_signal(tokio_utils::SignalKind::terminate()) => {
                    log::debug!("received SIGTERM");
                    self.send(Event::Shutdown).await;
                    true
                },
                msg = self.rx.recv() => {
                    match msg {
                        Some(Command::Start((h, p))) => {
                            log::debug!("got start signal for {}", p.name);
                            self.start(h, p).await;
                            true
                        }
                        Some(Command::Stop(h)) => {
                            self.stop(h).await;
                            true
                        },
                        None => {
                            log::debug!("channel closed");
                            false
                        }
                    }
                }
            };

            if !c {
                break;
            }
        }
        Ok(())
    }

    async fn start(&mut self, handle: NodeHandle, prog: config::Program) {
        log::debug!("starting program {}", prog.name);

        let (stdout, stderr) = (
            self.output_factory.stdout(&prog),
            self.output_factory.stderr(&prog),
        );

        let msg = match self.start_timeout {
            Some(t) => tokio_utils::with_timeout(start_program(prog, stdout, stderr), t).await,
            None => start_program(prog, stdout, stderr).await,
        };

        match msg {
            Ok(p) => {
                self.procs.insert(handle, p);
                self.send(Event::Started(handle)).await;
            }
            Err(e) => {
                self.send(Event::Err(e)).await;
            }
        }
    }

    async fn stop(&mut self, handle: NodeHandle) {
        match self.procs.remove(&handle) {
            None => {
                log::warn!("attempt to stop non-tracked process");
            }
            Some(proc) => {
                stop_program(proc, self.terminate_timeout).await;
                self.send(Event::Stopped(handle)).await;
            }
        };
    }

    async fn check_alive(&mut self) {
        let alive = |p: &Process| {
            if p.is_alive() {
                log::debug!("{} still alive", p);
                true
            } else {
                log::info!("{} stopped", p);
                false
            }
        };

        let stopped: std::collections::HashSet<NodeHandle> = self
            .procs
            .iter()
            .filter(|(_, p)| !alive(p))
            .map(|(h, _)| *h)
            .collect();

        for h in stopped.iter() {
            self.stop(*h).await;
        }
    }

    async fn send(&mut self, msg: Event) {
        self.tx.send(msg).await.expect("channel error");
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
    prog: config::Program,
    stdout: output::Sender,
    stderr: output::Sender,
) -> tokio_utils::Result<Process> {
    use config::ReadySignal;

    if prog.disabled {
        log::info!("{} disabled, not starting", prog.name);
        let info = ProcessInfo {
            name: prog.name,
            pid: 0,
        };
        let proc = Process::new(None, info, false);
        return Ok(proc);
    }

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

    let child = process::Command::new(executable)
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

async fn stop_program(proc: Process, timeout: Duration) {
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

async fn terminate_wait(
    mut proc: Process,
    timeout: Duration,
) -> std::result::Result<Option<std::process::ExitStatus>, Box<dyn std::error::Error>> {
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

fn terminate(pid: u32) -> std::result::Result<(), Box<dyn std::error::Error>> {
    use nix::sys::signal as nix_signal;

    let pid = nix::unistd::Pid::from_raw(pid as i32);
    let sig = nix_signal::Signal::SIGTERM;

    nix_signal::kill(pid, sig).map_err(|e| e.into())
}

fn is_alive(pid: u32) -> bool {
    use nix::sys::wait;

    let pid = nix::unistd::Pid::from_raw(pid as i32);
    match wait::waitpid(pid, Some(wait::WaitPidFlag::WNOHANG)) {
        Ok(wait::WaitStatus::StillAlive) => true,
        _ => false,
    }
}

fn exit_status(pid: u32) -> Option<i32> {
    use nix::sys::wait;

    let pid = nix::unistd::Pid::from_raw(pid as i32);
    match wait::waitpid(pid, None) {
        Ok(wait::WaitStatus::Exited(_, code)) => Some(code),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    extern crate regex;

    #[tokio::test]
    async fn is_alive_and_stop() {
        let proc = Process::new_with_name(
            process::Command::new("/bin/cat")
                .kill_on_drop(true)
                .spawn()
                .expect("cat"),
            "cat".to_string(),
            false,
        );
        let pid = proc.info.pid;

        assert!(proc.is_alive());

        let timeout = Duration::from_millis(1);
        stop_program(proc, timeout).await;

        assert!(!is_alive(pid));
    }

    #[tokio::test]
    async fn exit_status_good() {
        let proc = Process::new_with_name(
            process::Command::new("/bin/ls")
                .kill_on_drop(true)
                .stdout(std::process::Stdio::null())
                .stderr(std::process::Stdio::null())
                .spawn()
                .expect("ls"),
            "ls".to_string(),
            false,
        );
        let pid = proc.info.pid;

        assert_eq!(0, exit_status(pid).unwrap());
    }

    #[tokio::test]
    async fn exit_status_bad() {
        let proc = Process::new_with_name(
            process::Command::new("/bin/ls")
                .arg("path_does_not_exists")
                .kill_on_drop(true)
                .stdout(std::process::Stdio::null())
                .stderr(std::process::Stdio::null())
                .spawn()
                .expect("ls"),
            "ls".to_string(),
            false,
        );
        let pid = proc.info.pid;

        assert_ne!(0, exit_status(pid).unwrap());
    }

    #[tokio::test]
    async fn format_process() {
        let re = regex::Regex::new("catname:[0-9]+").expect("re");

        let proc = Process::new_with_name(
            process::Command::new("/bin/cat")
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
