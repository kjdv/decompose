extern crate petgraph;
extern crate string_error;

use super::*;

use std::collections::HashMap;

use petgraph::dot::{Config, Dot};
use petgraph::Direction::Incoming;

type Result<T> = std::result::Result<T, Box<dyn Error>>;

pub trait Node {
    fn from_config(p: &config::Program) -> Self;
    fn is_ready(&self) -> bool {
        true
    }
}

pub struct Graph<T: Node> {
    graph: petgraph::Graph<T, i32>,
}

type NodeHandle = petgraph::prelude::NodeIndex<u32>;

impl<T: Node + std::fmt::Display> Graph<T> {
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

    pub fn node(&self, h: NodeHandle) -> &T {
        &self.graph[h]
    }

    pub fn start(&self) -> impl Iterator<Item = NodeHandle> + '_ {
        self.graph.externals(Incoming)
    }

    pub fn expand(&self, h: NodeHandle) -> impl Iterator<Item = NodeHandle> + '_ {
        let n = self.node(h);
        assert!(n.is_ready());

        self.graph.neighbors(h).filter(move |i| {
            self.graph
                .neighbors_directed(*i, Incoming)
                .all(|j| self.node(j).is_ready())
        })
    }

    pub fn dot(&self, w: &mut impl std::io::Write) {
        w.write_fmt(format_args!(
            "{}",
            Dot::with_config(&self.graph, &[Config::EdgeNoLabel])
        ))
        .expect("write");
    }

    fn validate(graph: &petgraph::Graph<T, i32>) -> Result<()> {
        assert!(graph.externals(Incoming).any(|_| true));
        Ok(())
    }
}

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
            .externals(Incoming)
            .map(|h| graph.node(h).name.clone())
            .collect();
        assert_eq!(entry_nodes, vec!["single".to_string()]);
    }

    #[test]
    fn construct_ensemble() {
        let cfg = r#"
        [[program]]
        name = "server"
        argv = ["server"]

        [[program]]
        name = "proxy"
        argv = ["proxy"]
        depends = ["server"]
        "#;

        let graph = make(cfg);

        let first_neigbours: Vec<String> = graph
            .graph
            .externals(Incoming)
            .map(|i| graph.graph.neighbors(i))
            .flatten()
            .map(|h| graph.node(h).name.clone())
            .collect();
        assert_eq!(first_neigbours, vec!["proxy".to_string()]);

        // lets see if we can go the other way as well
        let first_neigbours: Vec<String> = graph
            .graph
            .externals(petgraph::Direction::Outgoing)
            .map(|i| graph.graph.neighbors_directed(i, Incoming))
            .flatten()
            .map(|h| graph.node(h).name.clone())
            .collect();
        assert_eq!(first_neigbours, vec!["server".to_string()]);
    }
}
