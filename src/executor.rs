extern crate tokio;

use super::*;

use log;
use std::collections::HashMap;

use graph::{Graph, NodeHandle};
use tokio::process::Command;
use tokio::sync::mpsc;

type TokResult<T> = std::result::Result<T, tokio::io::Error>;
type Result<T> = std::result::Result<T, Box<dyn Error>>;
type Process = Box<tokio::process::Child>;

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

                            tokio::spawn(start_program(n, p, tx));
                        });
                }
                Err(e) => {
                    return Err(e.into());
                }
            };
        }

        Ok(())
    }

    pub async fn stop(&mut self) {
        let (tx, mut rx) = mpsc::channel(100);

        let leaves: Vec<_> = self.dependency_graph.leaves().collect();
        leaves.iter().for_each(|h| {
            let op = self.running.get_mut(&h).expect("no process for node");
            if let Some(p) = op.take() {
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
                    tokio::spawn(stop_program(*h, p, self.terminate_timeout, tx.clone()));
                }
            });

            self.running.remove(&h);
        }

        assert!(self.running.is_empty());
    }
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
    let child = Command::new("echo")
        .arg("hello")
        .arg("world")
        .kill_on_drop(true)
        .spawn()?;

    tokio::time::delay_for(tokio::time::Duration::from_secs(1)).await;
    log::info!("{} is ready", prog.name);

    Ok(Box::new(child))
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
    // todo: graceful stop
    // for now: killed on drop
}

#[cfg(test)]
mod tests {
    // use super::*;

    #[test]
    fn placeholder() {
        assert!(true);
    }
}
