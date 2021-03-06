extern crate nix;
extern crate tokio;

use super::config;

use super::graph::{Graph, NodeHandle};
use super::process;
use std::collections::HashSet;

use process::mpsc;
use process::Command;
use process::Event;

type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;

pub struct Executor {
    dependency_graph: Graph,
    tx: process::mpsc::Sender<Command>,
    rx: process::mpsc::Receiver<Event>,

    // todo: this tracks a lot of state, in a fiddly way. Partially
    // because of different required behavior when starting up, shutting down..
    // refactoring ideas:
    //   1. turn this into a state machine (init->run->shutdown)
    //   2. similar, but each state in just one (private) method. Keep state variables on the function scope
    running: HashSet<NodeHandle>,
    pending: HashSet<NodeHandle>,
    shutting_down: bool,
    status: Option<ExitStatus>,
}

impl Executor {
    pub fn from_config(
        cfg: &config::System,
        tx: process::mpsc::Sender<Command>,
        rx: process::mpsc::Receiver<Event>,
    ) -> Result<Executor> {
        let graph = Graph::from_config(&cfg)?;

        Ok(Executor {
            dependency_graph: graph,
            tx,
            rx,
            running: HashSet::new(),
            pending: HashSet::new(),
            shutting_down: false,
            status: None,
        })
    }

    pub async fn run(mut self) -> Result<()> {
        log::info!("starting execution");

        self.init().await?;

        while let Some(event) = self.rx.recv().await {
            if !self.process(event).await? || !self.is_alive() {
                break;
            }
        }
        log::debug!("broken from event loop");

        self.shutdown().await?;

        log::info!("stopping execution");
        match self.status {
            None => Ok(()),
            Some(status) => status.into_result(),
        }
    }

    async fn process(&mut self, event: Event) -> Result<bool> {
        log::debug!("processing event");

        match event {
            Event::Started(h) => {
                self.on_started(h).await;
                Ok(true)
            }
            Event::Stopped(h, s) => {
                self.on_stopped(h, s).await;
                Ok(true)
            }
            Event::Shutdown => {
                self.shutdown().await?;
                Ok(true)
            }
            Event::Err(e) => {
                log::error!("{}", e);
                Err(e.into())
            }
        }
    }

    #[allow(dead_code)] // surpress false warning, used in tests
    fn is_alive(&self) -> bool {
        !self.pending.is_empty() || !self.running.is_empty()
    }

    async fn init(&mut self) -> Result<()> {
        self.pending = self.dependency_graph.all().collect();
        self.status = None;

        for h in self.dependency_graph.roots() {
            self.send_start(h).await;
        }
        Ok(())
    }

    async fn shutdown(&mut self) -> Result<()> {
        log::debug!("initiating shutdown");

        self.shutting_down = true;

        if self.is_alive() {
            for h in self.dependency_graph.leaves() {
                self.send_stop(h).await;
            }
        }
        Ok(())
    }

    async fn on_started(&mut self, handle: NodeHandle) {
        self.pending.remove(&handle);
        self.running.insert(handle);

        for h in self.dependency_graph.expand(handle, |n| {
            self.running.contains(&n) || !self.pending.contains(&n)
        }) {
            self.send_start(h).await;
        }
    }

    async fn on_stopped(&mut self, handle: NodeHandle, status: Option<process::ExitStatus>) {
        if let Some(h) = self.running.take(&handle) {
            let p = self.dependency_graph.node(h);
            log::debug!("on stopped for {} {}", p.name, p.critical);
            if p.critical && !p.disabled {
                log::info!("critical task {} stopped", p.name);

                if self.status.is_none() && status.is_some() {
                    self.status = Some(ExitStatus {
                        name: p.name.clone(),
                        status: status.unwrap(),
                    });
                }

                let _ = self.shutdown().await;
            }
        }

        if self.shutting_down {
            for h in self
                .dependency_graph
                .expand_back(handle, |n| !self.running.contains(&n))
            {
                self.send_stop(h).await;
            }
        }
    }

    async fn send_start(&self, handle: NodeHandle) {
        let p = self.dependency_graph.node(handle).clone();

        log::info!("starting program {}", p.name);
        let cmd = Command::Start((handle, p));
        self.send(cmd).await;
    }

    async fn send_stop(&self, handle: NodeHandle) {
        let p = self.dependency_graph.node(handle);

        log::info!("stopping program {}", p.name);
        let cmd = Command::Stop(handle);

        self.send(cmd).await;
    }

    async fn send(&self, cmd: Command) {
        Self::send_tx(self.tx.clone(), cmd).await;
    }

    async fn send_tx(mut tx: mpsc::Sender<Command>, cmd: Command) {
        tx.send(cmd).await.expect("channel error");
    }
}

struct ExitStatus {
    name: String,
    status: process::ExitStatus,
}

impl ExitStatus {
    fn into_result(self) -> Result<()> {
        match self.status.success() {
            true => Ok(()),
            false => Err(Box::new(ExitStatusError {
                name: self.name,
                status: self.status,
            })),
        }
    }
}

#[derive(Debug)]
struct ExitStatusError {
    name: String,
    status: process::ExitStatus,
}

impl std::fmt::Display for ExitStatusError {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        assert!(!self.status.success());
        write!(f, "{} failed: {}", self.name, self.status)
    }
}

impl std::error::Error for ExitStatusError {}

#[cfg(test)]
mod tests {
    use super::super::tokio_utils;
    use super::*;

    const TIMEOUT: std::time::Duration = std::time::Duration::from_millis(5);

    struct Fixture {
        rx: mpsc::Receiver<Command>,
        exec: Executor,
    }

    impl Fixture {
        fn new(toml: &str) -> Result<Fixture> {
            let cfg = config::System::from_toml(toml)?;

            let (_, status_rx) = mpsc::channel(10);
            let (cmd_tx, cmd_rx) = mpsc::channel(10);

            let exec = Executor::from_config(&cfg, cmd_tx, status_rx)?;
            Ok(Fixture { rx: cmd_rx, exec })
        }

        async fn recv(&mut self) -> Command {
            tokio::select! {
                _ = tokio::time::delay_for(TIMEOUT) => {
                    panic!("timeout");
                },
                x = self.rx.recv() => {
                    match x {
                        None => {
                            panic!("channel error")
                        },
                        Some(cmd) => cmd,
                    }
                }
            }
        }

        async fn expect_start(&mut self, name: &str) -> NodeHandle {
            match self.recv().await {
                Command::Start((h, p)) => {
                    assert_eq!(name, p.name);
                    h
                }
                _ => panic!("unexpected message"),
            }
        }

        async fn expect_stop(&mut self, handle: NodeHandle) {
            match self.recv().await {
                Command::Stop(h) => {
                    assert_eq!(h, handle);
                }
                _ => panic!("unexpected message"),
            }
        }

        async fn expect_nothing(&mut self) {
            tokio::select! {
                _ = tokio::time::delay_for(TIMEOUT) => (),
                _ = self.rx.recv() => {
                    panic!("unexpected message")
                }
            };
        }
    }

    #[tokio::test]
    async fn start_single() {
        let toml = r#"
        [[program]]
        name = "single"
        exec = "e"
        "#;

        let mut fixture = Fixture::new(toml).unwrap();
        fixture.exec.init().await.unwrap();

        fixture.expect_start("single").await;
        fixture.expect_nothing().await;
    }

    #[tokio::test]
    async fn depencencies_are_unlocked_on_started() {
        let toml = r#"
        [[program]]
        name = "a"
        exec = "e"

        [[program]]
        name = "b"
        exec = "e"

        [[program]]
        name = "c"
        exec = "e"
        depends = ["a", "b"]
        "#;

        let mut fixture = Fixture::new(toml).unwrap();
        fixture.exec.init().await.unwrap();

        let a = fixture.expect_start("a").await;
        let b = fixture.expect_start("b").await;
        fixture.expect_nothing().await;

        fixture.exec.process(Event::Started(a)).await.unwrap();
        fixture.expect_nothing().await;

        fixture.exec.process(Event::Started(b)).await.unwrap();
        fixture.expect_start("c").await;
        fixture.expect_nothing().await;
    }

    #[tokio::test]
    async fn error_stops_process() {
        let toml = r#"
        [[program]]
        name = "single"
        exec = "e"
        "#;

        let mut fixture = Fixture::new(toml).unwrap();
        fixture
            .exec
            .process(Event::Err(tokio_utils::make_err("bad")))
            .await
            .expect_err("expect err");
    }

    #[tokio::test]
    async fn alive_is_false_if_everything_is_stopped() {
        let toml = r#"
        [[program]]
        name = "a"
        exec = "e"

        [[program]]
        name = "b"
        exec = "e"
        "#;

        let mut fixture = Fixture::new(toml).unwrap();
        assert!(!fixture.exec.is_alive());

        fixture.exec.init().await.unwrap();
        let a = fixture.expect_start("a").await;
        let b = fixture.expect_start("b").await;
        fixture.exec.process(Event::Started(a)).await.unwrap();
        fixture.exec.process(Event::Started(b)).await.unwrap();
        assert!(fixture.exec.is_alive());

        fixture.exec.process(Event::Stopped(a, None)).await.unwrap();
        assert!(fixture.exec.is_alive());

        fixture.exec.process(Event::Stopped(b, None)).await.unwrap();
        assert!(!fixture.exec.is_alive());
    }

    #[tokio::test]
    async fn stopping_critical_process_initiates_shutdown() {
        let toml = r#"
        [[program]]
        name = "a"
        exec = "e"

        [[program]]
        name = "b"
        exec = "e"
        critical = false

        [[program]]
        name = "c"
        exec = "e"
        critical = true
        "#;

        let mut fixture = Fixture::new(toml).unwrap();
        fixture.exec.init().await.unwrap();
        let a = fixture.expect_start("a").await;
        let b = fixture.expect_start("b").await;
        let c = fixture.expect_start("c").await;
        fixture.exec.process(Event::Started(a)).await.unwrap();
        fixture.exec.process(Event::Started(b)).await.unwrap();
        fixture.exec.process(Event::Started(c)).await.unwrap();

        assert!(fixture.exec.process(Event::Stopped(b, None)).await.unwrap());
        assert!(fixture.exec.process(Event::Stopped(c, None)).await.unwrap());

        fixture.expect_stop(a).await;
        fixture.expect_stop(b).await;
        fixture.expect_stop(c).await;
    }

    #[tokio::test]
    async fn send_stop_while_not_shutting_down_has_no_further_effect() {
        let toml = r#"
        [[program]]
        name = "a"
        exec = "e"

        [[program]]
        name = "b"
        exec = "e"
        "#;

        let mut fixture = Fixture::new(toml).unwrap();
        fixture.exec.init().await.unwrap();
        let a = fixture.expect_start("a").await;
        fixture.expect_start("b").await;

        fixture.exec.process(Event::Stopped(a, None)).await.unwrap();
        fixture.expect_nothing().await;
    }

    #[tokio::test]
    async fn send_stop_while_shutting_down_sends_stop_commands() {
        let toml = r#"
        [[program]]
        name = "a"
        exec = "e"

        [[program]]
        name = "b"
        exec = "e"
        depends = ["a"]
        "#;

        let mut fixture = Fixture::new(toml).unwrap();
        fixture.exec.init().await.unwrap();
        let a = fixture.expect_start("a").await;
        fixture.exec.process(Event::Started(a)).await.unwrap();
        let b = fixture.expect_start("b").await;

        fixture.exec.shutdown().await.unwrap();
        fixture.expect_stop(b).await;
        fixture.expect_nothing().await;

        fixture.exec.process(Event::Stopped(b, None)).await.unwrap();
        fixture.expect_stop(a).await;
    }

    #[tokio::test]
    async fn shutting_down_while_no_longer_alive_has_no_effect() {
        let toml = r#"
        [[program]]
        name = "a"
        exec = "e"
        "#;

        let mut fixture = Fixture::new(toml).unwrap();
        fixture.exec.init().await.unwrap();
        let a = fixture.expect_start("a").await;
        fixture.exec.process(Event::Started(a)).await.unwrap();
        fixture.exec.process(Event::Stopped(a, None)).await.unwrap();

        assert!(!fixture.exec.is_alive());

        fixture.exec.shutdown().await.unwrap();
        fixture.expect_nothing().await;
    }

    #[tokio::test]
    async fn temporarily_nothing_running_is_allowed_during_startup() {
        let toml = r#"
        [[program]]
        name = "a"
        exec = "e"

        [[program]]
        name = "b"
        exec = "e"
        depends = ["a"]
        "#;

        let mut fixture = Fixture::new(toml).unwrap();
        fixture.exec.init().await.unwrap();

        let a = fixture.expect_start("a").await;
        fixture.exec.process(Event::Started(a)).await.unwrap();
        fixture.exec.process(Event::Stopped(a, None)).await.unwrap();

        assert!(fixture.exec.is_alive());
        fixture.expect_start("b").await;
    }

    #[tokio::test]
    async fn dependency_complete_before_start() {
        let toml = r#"
        [[program]]
        name = "a"
        exec = "e"
        ready = {completed={}}

        [[program]]
        name = "b"
        exec = "e"

        [[program]]
        name = "c"
        exec = "e"
        depends = ["a", "b"]
        "#;

        let mut fixture = Fixture::new(toml).unwrap();
        fixture.exec.init().await.unwrap();

        let a = fixture.expect_start("a").await;
        let b = fixture.expect_start("b").await;
        fixture.exec.process(Event::Started(a)).await.unwrap();
        fixture.exec.process(Event::Stopped(a, None)).await.unwrap();
        fixture.expect_nothing().await;

        fixture.exec.process(Event::Started(b)).await.unwrap();
        fixture.expect_start("c").await;
    }
}
