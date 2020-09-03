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
    running: HashMap<NodeHandle, Process>,
}

impl Executor {
    pub fn from_config(cfg: &config::System) -> Result<Executor> {
        let graph = Graph::from_config(&cfg)?;

        Ok(Executor {
            dependency_graph: graph,
            running: HashMap::new(),
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
                    self.running.insert(h, p);

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

#[cfg(test)]
mod tests {
    // use super::*;

    #[test]
    fn placeholder() {
        assert!(true);
    }
}
