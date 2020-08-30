extern crate petgraph;
extern crate string_error;

use super::*;

use petgraph::Direction;

type Result<T> = std::result::Result<T, Box<dyn Error>>;

#[derive(PartialEq, Eq, Debug, Clone)]
pub struct Node {
    name: String,
}

pub struct Graph {
    graph: petgraph::Graph<Node, ()>,
}

impl Graph {
    pub fn from_config(sys: config::System) -> Result<Graph> {
        let mut graph = petgraph::Graph::new();

        for prog in sys.program.iter() {
            graph.add_node(Node {
                name: prog.name.clone(),
            });
        }

        Graph::validate(&graph).map(|_| Graph { graph })
    }

    fn validate(graph: &petgraph::Graph<Node, ()>) -> Result<()> {
        assert!(graph.externals(Direction::Outgoing).any(|_| true));
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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

        let entry_nodes: Vec<Node> = graph
            .graph
            .externals(Direction::Outgoing)
            .map(|i| graph.graph[i].clone())
            .collect();
        assert_eq!(
            entry_nodes,
            vec![Node {
                name: "single".to_string()
            }]
        );
    }
}
