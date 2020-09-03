extern crate tokio;

use super::*;

use log;
use std::collections::HashMap;
use std::sync::Arc;

use graph::{Graph, NodeHandle};
use tokio::process::Command;
use tokio::sync::mpsc;

type Result<T> = std::result::Result<T, tokio::io::Error>;
type Process = Box<tokio::process::Child>;

pub async fn start_all(graph: &Graph) -> Result<HashMap<NodeHandle, Process>> {
    let mut result = HashMap::new();

    let (tx, mut rx) = mpsc::channel(100);

    graph.roots().for_each(|h| {
        let p = graph.node(h).clone();
        let tx = tx.clone();

        tokio::spawn(start_program(h, p, tx));
    });

    while let Some((h, p)) = rx.recv().await {
        result.insert(h, p);

        graph.expand(h, |i| result.contains_key(&i)).for_each(|n| {
            let p = graph.node(n).clone();
            let tx = tx.clone();

            tokio::spawn(start_program(n, p, tx));
        });
    }

    Ok(result)
}

async fn do_start_program(prog: config::Program) -> Result<Process> {
    let child = Command::new("echo")
        .arg("hello")
        .arg("world")
        .kill_on_drop(true)
        .spawn()?;

    tokio::time::delay_for(tokio::time::Duration::from_secs(1)).await;
    log::info!("{} is ready", prog.name);

    Ok(Box::new(child))
}

async fn start_program(
    h: NodeHandle,
    prog: config::Program,
    mut completed: mpsc::Sender<(NodeHandle, Process)>,
) {
    match do_start_program(prog).await {
        Ok(p) => {
            completed.send((h, p)).await.expect("channel error");
        }
        Err(e) => {
            log::error!("{}", e);
        }
    };
}

#[cfg(test)]
mod tests {
    // use super::*;

    #[test]
    fn placeholder() {
        assert!(true);
    }
}
