use petgraph::graph::DiGraph;


pub struct Problem<'r> {
    pub dfg: &'r DiGraph<usize, ()>,
    pub size: Vec<usize>,
}