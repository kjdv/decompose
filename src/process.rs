extern crate tokio;

use super::config;
use super::graph::NodeHandle;
use super::output;
use super::readysignals;
use super::tokio_utils;
pub use std::process::ExitStatus;
use std::time::Duration;
use tokio::process;
use tokio::sync::broadcast;
pub use tokio::sync::mpsc;

#[derive(Debug, Clone)]
pub enum Command {
    Start((NodeHandle, config::Program)),
    Stop(NodeHandle),
}

#[derive(Debug)]
pub enum Event {
    Started(NodeHandle),
    Stopped(NodeHandle, Option<ExitStatus>),
    Shutdown,
    Err(tokio::io::Error),
}

pub struct ProcessManager {
    rx: mpsc::Receiver<Command>,
    tx: mpsc::Sender<Event>,
    stop_tx: broadcast::Sender<NodeHandle>,
    output_factory: Box<dyn output::OutputFactory>,
    start_timeout: Option<Duration>,
    terminate_timeout: Duration,
}

impl ProcessManager {
    pub fn new(
        rx: mpsc::Receiver<Command>,
        tx: mpsc::Sender<Event>,
        sys: &config::System,
        output_factory: Box<dyn output::OutputFactory>,
    ) -> ProcessManager {
        let (stop_tx, _) = broadcast::channel(10);
        ProcessManager {
            rx,
            tx,
            stop_tx,
            output_factory,
            start_timeout: sys.start_timeout.map(Duration::from_secs_f64),
            terminate_timeout: Duration::from_secs_f64(sys.terminate_timeout),
        }
    }

    pub async fn run(mut self) -> std::result::Result<(), Box<dyn std::error::Error>> {
        loop {
            let c = tokio::select! {
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

        tokio::spawn(run_program(
            handle,
            prog,
            stdout,
            stderr,
            self.tx.clone(),
            self.stop_tx.subscribe(),
            self.start_timeout,
            self.terminate_timeout,
        ));
    }

    async fn stop(&mut self, handle: NodeHandle) {
        if let Err(e) = self.stop_tx.send(handle) {
            log::warn!("failed to forward stop command: {:?}", e);
        }
    }

    async fn send(&mut self, msg: Event) {
        if let Err(e) = self.tx.send(msg).await {
            log::debug!("channel error: {}", e);
        }
    }
}

#[derive(Debug, Clone)]
struct ProcessInfo {
    pub name: String,
    pub pid: u32,
}

impl std::fmt::Display for ProcessInfo {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        write!(f, "{}:{}", self.name, self.pid)
    }
}

async fn with_timeout<R>(
    f: impl futures::future::Future<Output = tokio_utils::Result<R>>,
    timeout: Option<Duration>,
) -> tokio_utils::Result<R> {
    match timeout {
        None => f.await,
        Some(timeout) => tokio_utils::with_timeout(f, timeout).await,
    }
}

async fn run_program(
    handle: NodeHandle,
    prog: config::Program,
    stdout: output::Sender,
    stderr: output::Sender,
    event_tx: mpsc::Sender<Event>,
    stop_rx: broadcast::Receiver<NodeHandle>,
    start_timeout: Option<std::time::Duration>,
    terminate_timeout: std::time::Duration,
) {
    let mut tx = event_tx.clone();
    if let Err(e) = do_run_program(
        handle,
        prog,
        stdout,
        stderr,
        event_tx,
        stop_rx,
        start_timeout,
        terminate_timeout,
    )
    .await
    {
        if let Err(e) = tx.send(Event::Err(e)).await {
            log::warn!("{}", e);
        }
    }
}

async fn do_run_program(
    handle: NodeHandle,
    prog: config::Program,
    stdout: output::Sender,
    stderr: output::Sender,
    mut event_tx: mpsc::Sender<Event>,
    stop_rx: broadcast::Receiver<NodeHandle>,
    start_timeout: Option<std::time::Duration>,
    terminate_timeout: std::time::Duration,
) -> tokio_utils::Result<()> {
    // bit of a monster function, but actually easiest to reason about to think of
    // a straight line of progression

    use config::ReadySignal;

    if prog.disabled {
        log::info!("{} disabled, not starting", prog.name);
        event_tx
            .send(Event::Started(handle))
            .await
            .map_err(tokio_utils::make_err)?;
        event_tx
            .send(Event::Stopped(handle, None))
            .await
            .map_err(tokio_utils::make_err)?;

        return Ok(());
    }

    log::debug!("{} creating child process", prog.name);
    let (mut proc, info) = create_child_process(&prog)?;

    log::info!("{} started", info);

    log::debug!("{} hooking up stop command", info);
    tokio::spawn(wait_for_stop_command(
        handle,
        info.clone(),
        terminate_timeout,
        stop_rx,
    ));

    log::debug!("{} hooking up output pipes", info);
    let monitor_out = stdout.subscribe();
    let monitor_err = stderr.subscribe();
    tokio::spawn(output::produce(stdout, proc.stdout.take()));
    tokio::spawn(output::produce(stderr, proc.stderr.take()));

    log::debug!("{} waiting for ready signal", info);

    if let ReadySignal::Completed = prog.ready {
        // special case
        let status = with_timeout(readysignals::completed(proc), start_timeout).await?;
        if status.success() {
            log::info!("{} ready", info);
            event_tx
                .send(Event::Started(handle))
                .await
                .map_err(tokio_utils::make_err)?;
            log::info!("{} stopped", info);
            event_tx
                .send(Event::Stopped(handle, Some(status)))
                .await
                .map_err(tokio_utils::make_err)?;
            return Ok(());
        } else {
            let msg = format!("{} not ready", info);
            log::error!("{}", msg);
            return Err(tokio_utils::make_err(msg));
        }
    }

    let rs = match prog.ready {
        ReadySignal::Nothing => with_timeout(readysignals::nothing(), start_timeout).await?,
        ReadySignal::Manual => {
            // not setting timeout on manual trigger
            readysignals::manual(info.name.as_str()).await?
        }
        ReadySignal::Timer(s) => {
            let dur = Duration::from_secs_f64(s);
            // not setting timeout on already time-based signal
            readysignals::timer(dur).await?
        }
        ReadySignal::Port(port) => with_timeout(readysignals::port(port), start_timeout).await?,
        ReadySignal::Stdout(re) => {
            with_timeout(
                readysignals::output(monitor_out, re.as_str()),
                start_timeout,
            )
            .await?
        }
        ReadySignal::Stderr(re) => {
            with_timeout(
                readysignals::output(monitor_err, re.as_str()),
                start_timeout,
            )
            .await?
        }
        ReadySignal::Healthcheck(endpoint) => {
            with_timeout(
                readysignals::healthcheck(
                    endpoint.host.as_str(),
                    endpoint.port,
                    endpoint.path.as_str(),
                ),
                start_timeout,
            )
            .await?
        }
        ReadySignal::Completed => panic!("not handled here"),
    };

    match rs {
        true => {
            log::info!("{} ready", info);
            event_tx
                .send(Event::Started(handle))
                .await
                .expect("event channel error");
        }
        false => {
            let msg = format!("{} not ready", info);
            log::error!("{}", msg);
            return Err(tokio_utils::make_err(msg));
        }
    }

    log::debug!("{} waiting for completion or stop signal", info);

    let output = proc.wait_with_output().await?;
    log::info!("{} stopped, {}", info, output.status);

    event_tx
        .send(Event::Stopped(handle, Some(output.status)))
        .await
        .expect("event channel error");

    Ok(())
}

async fn wait_for_stop_command(
    handle: NodeHandle,
    info: ProcessInfo,
    timeout: std::time::Duration,
    mut stop_rx: broadcast::Receiver<NodeHandle>,
) -> tokio_utils::Result<()> {
    while let Ok(h) = stop_rx.recv().await {
        if h == handle {
            log::debug!("{} received stop command", info);
            terminate(info.pid)?;

            tokio::time::delay_for(timeout).await;

            if is_alive(info.pid) {
                log::warn!("{} failed to terminate, killing", info);
                kill(info.pid)?;
            }
            break;
        }
    }
    Ok(())
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

fn terminate(pid: u32) -> tokio_utils::Result<()> {
    use nix::sys::signal as nix_signal;

    let pid = nix::unistd::Pid::from_raw(pid as i32);
    let sig = nix_signal::Signal::SIGTERM;

    nix_signal::kill(pid, sig).map_err(tokio_utils::make_err)
}

fn kill(pid: u32) -> tokio_utils::Result<()> {
    use nix::sys::signal as nix_signal;

    let pid = nix::unistd::Pid::from_raw(pid as i32);
    let sig = nix_signal::Signal::SIGKILL;

    nix_signal::kill(pid, sig).map_err(tokio_utils::make_err)
}

fn is_alive(pid: u32) -> bool {
    use nix::sys::wait;

    let pid = nix::unistd::Pid::from_raw(pid as i32);
    match wait::waitpid(pid, Some(wait::WaitPidFlag::WNOHANG)) {
        Ok(wait::WaitStatus::StillAlive) => true,
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn format_process() {
        let proc = ProcessInfo {
            name: "catname".to_string(),
            pid: 123,
        };

        let fmt = format!("{}", proc);
        assert_eq!("catname:123", fmt.as_str());
    }
}
