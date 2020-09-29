extern crate nix;
extern crate tokio;

use super::config;
use super::output;
use super::readysignals;
use super::tokio_utils;

use super::graph::{Graph, NodeHandle};
use super::process;
use nix::sys::signal as nix_signal;
use std::collections::HashSet;
use std::time::Duration;
use tokio::process::Command;
use tokio::signal::unix::SignalKind;
use tokio::sync::mpsc;

use process::ProcessCommandMessage;
use process::ProcessStatusMessage;

type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;

pub struct Executor {
    dependency_graph: Graph,
    tx: process::mpsc::Sender<ProcessCommandMessage>,
    rx: process::mpsc::Receiver<ProcessStatusMessage>,
    running: HashSet<NodeHandle>,
    shutdown: bool,
}

impl Executor {
    pub fn from_config(
        cfg: &config::System,
        tx: process::mpsc::Sender<ProcessCommandMessage>,
        rx: process::mpsc::Receiver<ProcessStatusMessage>,
    ) -> Result<Executor> {
        let graph = Graph::from_config(&cfg)?;

        Ok(Executor {
            dependency_graph: graph,
            tx,
            rx,
            running: HashSet::new(),
            shutdown: false,
        })
    }

    pub async fn run(&mut self) -> Result<()> {
        self.start().await;

        while !self.shutdown {
            tokio::select! {
                _ = tokio_utils::wait_for_signal(tokio_utils::SignalKind::interrupt()) => {
                    self.shutdown = true;
                },
                _ = tokio_utils::wait_for_signal(tokio_utils::SignalKind::terminate()) => {
                    self.shutdown = true;
                }
                msg = self.rx.recv() => {
                    match msg {
                        None => {
                            self.shutdown = true;
                        }
                        Some(msg) => {
                            self.shutdown = !self.process(msg).await
                        }
                    }
                }
            };
        }
        Ok(())
    }

    async fn process(&mut self, msg: ProcessStatusMessage) -> bool {
        log::debug!("processing msg");

        match msg {
            ProcessStatusMessage::Started(h) => {
                self.on_started(h).await;
                true
            }
            ProcessStatusMessage::Stopped(h) => {
                self.on_stopped(h).await;
                true
            }
            ProcessStatusMessage::AllStopped => false,
            ProcessStatusMessage::Err(e) => {
                log::error!("{}", e);
                false
            }
        }
    }

    async fn start(&mut self) {
        for h in self.dependency_graph.roots() {
            self.send_start(h).await;
        }
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
        let p = self.dependency_graph.node(handle);
        self.shutdown = p.critical;
    }

    async fn send_start(&self, handle: NodeHandle) {
        let p = self.dependency_graph.node(handle).clone();

        log::info!("starting program {}", p.name);
        let msg = ProcessCommandMessage::Start((handle, p));

        Self::send_tx(self.tx.clone(), msg).await;
    }

    async fn stop(&self) {}

    async fn send(&mut self, msg: ProcessCommandMessage) {
        self.tx.send(msg).await.expect("channel error");
    }

    async fn send_tx(mut tx: mpsc::Sender<ProcessCommandMessage>, msg: ProcessCommandMessage) {
        tx.send(msg).await.expect("channel error");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Fixture {
        tx: mpsc::Sender<ProcessStatusMessage>,
        rx: mpsc::Receiver<ProcessCommandMessage>,
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

        async fn recv(&mut self) -> ProcessCommandMessage {
            self.rx.recv().await.expect("channel error")
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
        fixture.exec.start().await;

        match fixture.recv().await {
            ProcessCommandMessage::Start((_, p)) => {
                assert_eq!(p.name, "single");
            }
            _ => {
                panic!("unexpected message");
            }
        }
    }
}
