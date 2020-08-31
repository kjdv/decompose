extern crate petgraph;
extern crate string_error;

use super::*;

use std::collections::HashMap;

use petgraph::dot::{Config, Dot};
use petgraph::Direction::{Incoming, Outgoing};

type Result<T> = std::result::Result<T, Box<dyn Error>>;

pub trait Node {
    fn from_config(p: &config::Program) -> Self;
    fn name(&self) -> &str;
    fn is_ready(&self) -> bool {
        true
    }
    fn is_stopped(&self) -> bool {
        true
    }
}

pub struct Graph<T: Node> {
    graph: petgraph::Graph<T, ()>,
}

type NodeHandle = petgraph::prelude::NodeIndex<u32>;

impl<T: Node> Graph<T> {
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
                graph.add_edge(*from, *to, ());
            }
        }

        Graph::validate(&graph)?;

        Ok(Graph { graph })
    }

    pub fn node(&self, h: NodeHandle) -> &T {
        &self.graph[h]
    }

    pub fn node_mut(&mut self, h: NodeHandle) -> &mut T {
        &mut self.graph[h]
    }

    pub fn start(&self) -> impl Iterator<Item = NodeHandle> + '_ {
        self.graph.externals(Incoming)
    }

    pub fn stop(&self) -> impl Iterator<Item = NodeHandle> + '_ {
        self.graph.externals(Outgoing)
    }

    pub fn expand(&self, h: NodeHandle) -> impl Iterator<Item = NodeHandle> + '_ {
        assert!(self.node(h).is_ready());

        self.dependees(h)
            .filter(move |i| self.dependencies(*i).all(|j| self.node(j).is_ready()))
    }

    pub fn expand_back(&self, h: NodeHandle) -> impl Iterator<Item = NodeHandle> + '_ {
        assert!(self.node(h).is_stopped());

        self.dependencies(h)
            .filter(move |i| self.dependees(*i).all(|j| self.node(j).is_stopped()))
    }

    pub fn dot(&self, w: &mut impl std::io::Write) {
        let m = self.graph.map(|_, n| n.name(), |_, _| 0);

        w.write_fmt(format_args!(
            "{}",
            Dot::with_config(&m, &[Config::EdgeNoLabel])
        ))
        .expect("write");
    }

    fn dependencies(&self, h: NodeHandle) -> impl Iterator<Item = NodeHandle> + '_ {
        self.graph.neighbors_directed(h, Incoming)
    }

    fn dependees(&self, h: NodeHandle) -> impl Iterator<Item = NodeHandle> + '_ {
        self.graph.neighbors(h)
    }

    fn validate(graph: &petgraph::Graph<T, ()>) -> Result<()> {
        assert!(graph.externals(Incoming).any(|_| true));
        Ok(())
    }
}

pub struct SimpleNode {
    name_: String,
}

impl Node for SimpleNode {
    fn from_config(p: &config::Program) -> Self {
        SimpleNode {
            name_: p.name.clone(),
        }
    }

    fn name(&self) -> &str {
        self.name_.as_str()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct TestNode {
        name_: String,
        ready: bool,
        stopped: bool,
    }

    impl Node for TestNode {
        fn from_config(p: &config::Program) -> Self {
            TestNode {
                name_: p.name.clone(),
                ready: false,
                stopped: false,
            }
        }

        fn name(&self) -> &str {
            self.name_.as_str()
        }

        fn is_ready(&self) -> bool {
            self.ready
        }

        fn is_stopped(&self) -> bool {
            self.stopped
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

        let entry_nodes: Vec<&str> = graph
            .graph
            .externals(Incoming)
            .map(|h| graph.node(h).name())
            .collect();
        assert_eq!(entry_nodes, vec!["single"]);
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

        let first_neigbours: Vec<&str> = graph
            .graph
            .externals(Incoming)
            .map(|i| graph.graph.neighbors(i))
            .flatten()
            .map(|h| graph.node(h).name())
            .collect();
        assert_eq!(first_neigbours, vec!["proxy"]);

        // lets see if we can go the other way as well
        let first_neigbours: Vec<&str> = graph
            .graph
            .externals(Outgoing)
            .map(|i| graph.graph.neighbors_directed(i, Incoming))
            .flatten()
            .map(|h| graph.node(h).name())
            .collect();
        assert_eq!(first_neigbours, vec!["server"]);
    }

    fn names<'a>(g: &'a Graph<TestNode>, hs: &Vec<NodeHandle>) -> Vec<&'a str> {
        hs.iter().map(|h| g.node(*h).name()).collect()
    }

    #[test]
    fn expand() {
        let cfg = r#"
        [[program]]
        name = "a"
        argv = ["a"]

        [[program]]
        name = "b"
        argv = ["b"]

        [[program]]
        name = "c"
        argv = ["c"]
        depends = ["a", "b"]

        [[program]]
        name = "d"
        argv = ["d"]
        depends = ["c"]
        "#;

        let mut graph = make(cfg);

        let start_nodes: Vec<NodeHandle> = graph.start().collect();
        assert_eq!(names(&graph, &start_nodes), vec!["a", "b"]);

        graph.node_mut(start_nodes[0]).ready = true;
        assert_eq!(0, graph.expand(start_nodes[0]).count());

        graph.node_mut(start_nodes[1]).ready = true;
        let expanded_nodes: Vec<NodeHandle> = graph.expand(start_nodes[1]).collect();
        assert_eq!(names(&graph, &expanded_nodes), vec!["c"]);

        graph.node_mut(expanded_nodes[0]).ready = true;
        let expanded_nodes: Vec<NodeHandle> = graph.expand(expanded_nodes[0]).collect();
        assert_eq!(names(&graph, &expanded_nodes), vec!["d"]);
    }

    #[test]
    fn expand_back() {
        let cfg = r#"
        [[program]]
        name = "a"
        argv = ["a"]

        [[program]]
        name = "b"
        argv = ["b"]

        [[program]]
        name = "c"
        argv = ["c"]
        depends = ["a", "b"]

        [[program]]
        name = "d"
        argv = ["d"]
        depends = ["c"]
        "#;

        let mut graph = make(cfg);

        let end_nodes: Vec<NodeHandle> = graph.stop().collect();
        assert_eq!(names(&graph, &end_nodes), vec!["d"]);

        graph.node_mut(end_nodes[0]).stopped = true;
        let expanded: Vec<NodeHandle> = graph.expand_back(end_nodes[0]).collect();
        assert_eq!(names(&graph, &expanded), vec!["c"]);

        graph.node_mut(expanded[0]).stopped = true;
        let expanded: Vec<NodeHandle> = graph.expand_back(expanded[0]).collect();
        assert_eq!(names(&graph, &expanded), vec!["b", "a"]);
    }
}
