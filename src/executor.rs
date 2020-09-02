extern crate tokio;

use super::*;

use log;
use std::collections::HashSet;

use futures::future::Future;
use graph::{Graph, NodeHandle};
use tokio::process::Command;
use tokio::sync::mpsc;

type Result<T> = std::result::Result<T, tokio::io::Error>;

async fn start_all(graph: &Graph) -> Result<()> {
    let (mut tx, mut rx) = mpsc::channel(100);

    graph.roots().for_each(|h| {
        let p = graph.node(h).clone();
        let tx = tx.clone();

        tokio::spawn(start_program(h, p, tx));
    });

    let mut visited = HashSet::new();
    while let Some(h) = rx.recv().await {
        visited.insert(h);
        graph.expand(h, |i| visited.contains(&i)).for_each(|n| {
            let p = graph.node(n).clone();
            let tx = tx.clone();

            tokio::spawn(start_program(n, p, tx));
        })
    }

    Ok(())
}

async fn do_start_program(prog: config::Program) -> Result<()> {
    let child = Command::new("echo")
        .arg("hello")
        .arg("world")
        .kill_on_drop(true)
        .spawn()?;

    tokio::time::delay_for(tokio::time::Duration::from_secs(1)).await;
    log::info!("{} is ready", prog.name);

    Ok(())
}

async fn start_program(
    h: NodeHandle,
    prog: config::Program,
    mut completed: mpsc::Sender<NodeHandle>,
) {
    match do_start_program(prog).await {
        Ok(()) => completed.send(h).await.expect("channel error"),
        Err(e) => log::error!("{}", e),
    };
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn placeholder() {
        assert!(true);
    }
}
