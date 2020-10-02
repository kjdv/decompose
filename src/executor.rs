extern crate nix;
extern crate tokio;

use super::config;
use super::output;
use super::tokio_utils;

use super::graph::{Graph, NodeHandle};
use super::process;
use std::collections::HashSet;
use std::time::Duration;

use process::mpsc;
use process::Command;
use process::Event;

type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;

pub struct Executor {
    dependency_graph: Graph,
    tx: process::mpsc::Sender<Command>,
    rx: process::mpsc::Receiver<Event>,
    running: HashSet<NodeHandle>,
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
        })
    }

    pub async fn run(&mut self) -> Result<()> {
        self.init().await?;

        while let Some(event) = self.rx.recv().await {
            self.process(event).await;
        }

        self.shutdown().await?;
        Ok(())
    }

    async fn process(&mut self, event: Event) -> bool {
        log::debug!("processing event");

        match event {
            Event::Started(h) => {
                self.on_started(h).await;
                true
            }
            Event::Stopped(h) => {
                self.on_stopped(h).await;
                true
            }
            Event::AllStopped => false,
            Event::Shutdown => false,
            Event::Err(e) => {
                log::error!("{}", e);
                false
            }
        }
    }

    async fn init(&mut self) -> Result<()> {
        for h in self.dependency_graph.roots() {
            self.send_start(h).await;
        }
        Ok(())
    }

    async fn shutdown(&mut self) -> Result<()> {
        Ok(())
    }

    async fn on_started(&mut self, handle: NodeHandle) {
        self.running.insert(handle);

        for h in self
            .dependency_graph
            .expand(handle, |n| self.running.contains(&n))
        {
            self.send_start(h).await;
        }
    }

    async fn on_stopped(&mut self, handle: NodeHandle) {
        self.running.remove(&handle);
    }

    async fn send_start(&self, handle: NodeHandle) {
        let p = self.dependency_graph.node(handle).clone();

        log::info!("starting program {}", p.name);
        let cmd = Command::Start((handle, p));

        Self::send_tx(self.tx.clone(), cmd).await;
    }

    async fn stop(&self) {}

    async fn send(&self, cmd: Command) {
        Self::send_tx(self.tx.clone(), cmd);
    }

    async fn send_tx(mut tx: mpsc::Sender<Command>, cmd: Command) {
        tx.send(cmd).await.expect("channel error");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const TIMEOUT: std::time::Duration = std::time::Duration::from_millis(5);

    struct Fixture {
        tx: mpsc::Sender<Event>,
        rx: mpsc::Receiver<Command>,
        exec: Executor,
    }

    impl Fixture {
        fn new(toml: &str) -> Result<Fixture> {
            let cfg = config::System::from_toml(toml)?;

            let (statux_tx, status_rx) = mpsc::channel(10);
            let (cmd_tx, cmd_rx) = mpsc::channel(10);

            let exec = Executor::from_config(&cfg, cmd_tx, status_rx)?;
            Ok(Fixture {
                tx: statux_tx,
                rx: cmd_rx,
                exec,
            })
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
}
