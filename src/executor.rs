extern crate tokio;

use super::*;

use futures::future::Future;
use std::error::Error;
use tokio::process::Command;

type Result<T> = std::result::Result<T, Box<dyn Error>>;
type Fut<T> = std::pin::Pin<std::boxed::Box<dyn Future<Output = T>>>;


struct ExecutionNode {
    program_definition: config::Program,
    child: Option<tokio::process::Child>
}

impl graph::Node for ExecutionNode {
    fn from_config(p: &config::Program) -> ExecutionNode {
        ExecutionNode{program_definition: p.clone(), child: None}
    }

    fn name(&self) -> &str {
        self.program_definition.name.as_str()
    }
}

impl ExecutionNode {
    pub fn start(&mut self) -> Result<Fut<()>> {
        let child = Command::new("echo").arg("hello").arg("world")
            .kill_on_drop(true)
            .spawn()?;
        self.child.replace(child);

        Ok(Box::pin(async move {
            tokio::time::delay_for(tokio::time::Duration::from_secs(1)).await;
        }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn placeholder() {
        assert!(false);
    }
}
