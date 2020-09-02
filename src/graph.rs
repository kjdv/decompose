extern crate petgraph;
extern crate string_error;

use super::*;

use std::collections::HashMap;

use petgraph::dot::{Config, Dot};
use petgraph::Direction::{Incoming, Outgoing};

type Result<T> = std::result::Result<T, Box<dyn Error>>;

pub struct Graph {
    graph: petgraph::Graph<config::Program, ()>,
}

pub type NodeHandle = petgraph::prelude::NodeIndex<u32>;

impl Graph {
    pub fn from_config(sys: config::System) -> Result<Graph> {
        let mut graph = petgraph::Graph::new();

        let mut mapping = HashMap::new();

        for prog in sys.program.iter() {
            let n = graph.add_node(prog.clone());
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

    pub fn node(&self, h: NodeHandle) -> &config::Program {
        &self.graph[h]
    }

    pub fn roots(&self) -> impl Iterator<Item = NodeHandle> + '_ {
        self.graph.externals(Incoming)
    }

    pub fn leaves(&self) -> impl Iterator<Item = NodeHandle> + '_ {
        self.graph.externals(Outgoing)
    }

    pub fn expand<'a, F>(
        &'a self,
        h: NodeHandle,
        visited: F,
    ) -> impl Iterator<Item = NodeHandle> + 'a
    where
        F: Fn(NodeHandle) -> bool + 'a,
    {
        self.dependees(h)
            .filter(move |i| self.dependencies(*i).all(&visited))
    }

    pub fn expand_back<'a, F>(
        &'a self,
        h: NodeHandle,
        visited: F,
    ) -> impl Iterator<Item = NodeHandle> + 'a
    where
        F: Fn(NodeHandle) -> bool + 'a,
    {
        self.dependencies(h)
            .filter(move |i| self.dependees(*i).all(&visited))
    }

    pub fn dot(&self, w: &mut impl std::io::Write) {
        let m = self.graph.map(|_, n| n.name.as_str(), |_, _| 0);

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

    fn validate(graph: &petgraph::Graph<config::Program, ()>) -> Result<()> {
        if !graph.externals(Incoming).next().is_some() {
            return Err(string_error::static_err(
                "system graph has no dependency-free root nodes",
            ));
        }

        if petgraph::algo::is_cyclic_directed(graph) {
            return Err(string_error::static_err("system graph contains cycles"));
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    fn make(toml: &str) -> Graph {
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

        let entry_nodes: Vec<_> = graph
            .graph
            .externals(Incoming)
            .map(|h| graph.node(h).name.clone())
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

        let first_neigbours: Vec<_> = graph
            .graph
            .externals(Incoming)
            .map(|i| graph.graph.neighbors(i))
            .flatten()
            .map(|h| graph.node(h).name.clone())
            .collect();
        assert_eq!(first_neigbours, vec!["proxy"]);

        // lets see if we can go the other way as well
        let first_neigbours: Vec<_> = graph
            .graph
            .externals(Outgoing)
            .map(|i| graph.graph.neighbors_directed(i, Incoming))
            .flatten()
            .map(|h| graph.node(h).name.clone())
            .collect();
        assert_eq!(first_neigbours, vec!["server"]);
    }

    #[test]
    fn cyclic_graph_fails_to_construct() {
        let toml = r#"
        [[program]]
        name = "a"
        argv = ["a"]

        [[program]]
        name = "b"
        argv = ["b"]
        depends = ["c"]

        [[program]]
        name = "c"
        argv = ["c"]
        depends = ["b"]
        "#;

        let cfg = config::System::from_toml(toml).unwrap();
        let g = Graph::from_config(cfg);
        assert!(g.is_err());
    }

    fn names(g: &Graph, hs: &Vec<NodeHandle>) -> Vec<String> {
        hs.iter().map(|h| g.node(*h).name.clone()).collect()
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

        let graph = make(cfg);
        let mut visited = HashSet::new();

        let start_nodes: Vec<NodeHandle> = graph.roots().collect();
        assert_eq!(names(&graph, &start_nodes), vec!["a", "b"]);

        visited.insert(start_nodes[0]);
        assert_eq!(
            0,
            graph
                .expand(start_nodes[0], |h| visited.contains(&h))
                .count()
        );

        visited.insert(start_nodes[1]);
        let expanded_nodes: Vec<NodeHandle> = graph
            .expand(start_nodes[1], |h| visited.contains(&h))
            .collect();
        assert_eq!(names(&graph, &expanded_nodes), vec!["c"]);

        visited.insert(expanded_nodes[0]);
        let expanded_nodes: Vec<NodeHandle> = graph
            .expand(expanded_nodes[0], |h| visited.contains(&h))
            .collect();
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

        let graph = make(cfg);
        let mut visited = HashSet::new();

        let end_nodes: Vec<NodeHandle> = graph.leaves().collect();
        assert_eq!(names(&graph, &end_nodes), vec!["d"]);

        visited.insert(end_nodes[0]);
        let expanded: Vec<NodeHandle> = graph
            .expand_back(end_nodes[0], |h| visited.contains(&h))
            .collect();
        assert_eq!(names(&graph, &expanded), vec!["c"]);

        visited.insert(expanded[0]);
        let expanded: Vec<NodeHandle> = graph
            .expand_back(expanded[0], |h| visited.contains(&h))
            .collect();
        assert_eq!(names(&graph, &expanded), vec!["b", "a"]);
    }
}
