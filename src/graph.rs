extern crate petgraph;
extern crate string_error;

use super::*;

use std::collections::HashMap;

use petgraph::dot::{Config, Dot};
use petgraph::Direction::{Incoming, Outgoing};

type Result<T> = std::result::Result<T, Box<dyn Error>>;

pub trait Node {
    fn from_config(p: &config::Program) -> Self;
}

pub struct Graph<T: Node> {
    graph: petgraph::Graph<T, i32>,
}

impl<T: Node + std::fmt::Display + std::fmt::Debug> Graph<T> {
    pub fn from_config(sys: config::System) -> Result<Graph<T>> {
        let mut graph = petgraph::Graph::new();

        let mut mapping = HashMap::new();

        for prog in sys.program.iter() {
            let n = graph.add_node(T::from_config(&prog));
            mapping.insert(prog.name.as_str(), n);
        }

        for prog in sys.program.iter() {
            for dep in prog.depends.iter() {
                let from = mapping
                    .get(dep.as_str())
                    .ok_or_else(|| string_error::into_err(format!("No such program: {}", dep)))?;
                let to = mapping.get(prog.name.as_str()).unwrap();
                graph.add_edge(*from, *to, 0);
            }
        }

        Graph::validate(&graph)?;

        Ok(Graph { graph })
    }

    pub fn dot(&self, w: &mut impl std::io::Write) {
        w.write_fmt(format_args!(
            "{}",
            Dot::with_config(&self.graph, &[Config::EdgeNoLabel])
        ))
        .expect("write");
    }

    fn validate(graph: &petgraph::Graph<T, i32>) -> Result<()> {
        assert!(graph.externals(Outgoing).any(|_| true));
        Ok(())
    }
}

#[derive(Debug)]
pub struct SimpleNode {
    name: String,
}

impl Node for SimpleNode {
    fn from_config(p: &config::Program) -> Self {
        SimpleNode {
            name: p.name.clone(),
        }
    }
}

impl std::fmt::Display for SimpleNode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.name)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Debug)]
    struct TestNode {
        name: String,
    }

    impl Node for TestNode {
        fn from_config(p: &config::Program) -> Self {
            TestNode {
                name: p.name.clone(),
            }
        }
    }

    impl std::fmt::Display for TestNode {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            write!(f, "{}", self.name)
        }
    }

    fn make(toml: &str) -> Graph<TestNode> {
        let cfg = config::System::from_toml(toml).unwrap();
        Graph::from_config(cfg).unwrap()
    }

    #[test]
    fn construct_single() {
        let cfg = r#"
        [[program]]
        name = "single"
        argv = ["blah"]
        "#;

        let graph = make(cfg);
        assert_eq!(1, graph.graph.node_count());

        let entry_nodes: Vec<String> = graph
            .graph
            .externals(Outgoing)
            .map(|i| graph.graph[i].name.clone())
            .collect();
        assert_eq!(entry_nodes, vec!["single".to_string()]);
    }
}
