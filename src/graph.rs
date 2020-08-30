pub struct Node<'a, T> {
    this: T,
    depends: Vec<&'a T>,
    dependees: Vec<&'a T>
}

pub struct Graph<'a, T> {
    nodes: Vec<Node<'a, T>>
}



#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn placeholder() {
        assert!(false);
    }
}
