extern crate petgraph;
extern crate string_error;

use super::*;

use petgraph::Direction;

type Result<T> = std::result::Result<T, Box<dyn Error>>;

pub trait Node {
    fn from_config(p: &config::Program) -> Self;
}

pub struct Graph<T: Node> {
    graph: petgraph::Graph<T, ()>,
}

impl<T: Node> Graph<T> {
    pub fn from_config(sys: config::System) -> Result<Graph<T>> {
        let mut graph = petgraph::Graph::new();

        for prog in sys.program.iter() {
            graph.add_node(T::from_config(&prog));
        }

        Graph::validate(&graph)?;

        Ok(Graph { graph })
    }

    fn validate(graph: &petgraph::Graph<T, ()>) -> Result<()> {
        assert!(graph.externals(Direction::Outgoing).any(|_| true));
        Ok(())
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
            .externals(Direction::Outgoing)
            .map(|i| graph.graph[i].name.clone())
            .collect();
        assert_eq!(entry_nodes, vec!["single".to_string()]);
    }
}
