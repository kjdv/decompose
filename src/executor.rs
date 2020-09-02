extern crate tokio;

use super::*;

use log;
use string_error;
use std::collections::HashSet;

use futures::future::{Future, BoxFuture, FutureExt};
use std::error::Error;
use tokio::process::Command;
use tokio::sync::{oneshot, mpsc};
use graph::{Graph, NodeHandle};

type Result<T> = std::result::Result<T, Box<dyn Error>>;
type Fut<T> = std::pin::Pin<std::boxed::Box<dyn Future<Output = T>>>;

async fn start_program(prog: config::Program) -> Result<()> {
    let child = Command::new("echo")
        .arg("hello")
        .arg("world")
        .kill_on_drop(true)
        .spawn()?;

    tokio::time::delay_for(tokio::time::Duration::from_secs(1)).await;
    log::info!("{} is ready", prog.name);

    Ok(())
}


async fn start_all(graph: &Graph) -> Result<()> {
    let (mut tx, mut rx) = mpsc::channel(100);

    graph.roots().for_each(|h| {
        let p = graph.node(h).clone();
        let mut tx = tx.clone();
        tokio::spawn(async move {
            if let Err(e) = start_program(p).await {
                log::error!("{}", e);
            }
            tx.send(h).await.unwrap();
        });
    });

    let mut visited = HashSet::new();
    while let Some(h) = rx.recv().await {
        visited.insert(h);
        graph.expand(h, |i| visited.contains(&i)).for_each(|n| {
            let p = graph.node(n).clone();
            let mut tx = tx.clone();

            tokio::spawn(async move {
                if let Err(e) = start_program(p).await {
                    log::error!("{}", e);
                }
                tx.send(h).await.unwrap();
            });
        })
    }

    Ok(())
}


#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn placeholder() {
        assert!(true);
    }
}
