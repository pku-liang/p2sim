use std::{
    fs::File,
    io::{BufWriter, Write},
    path::PathBuf,
};

use petgraph::{
    Direction,
    graph::{DiGraph, NodeIndex, node_index},
};

use crate::ir::*;

pub fn compute_inst_costs(insts: &[Inst]) -> f64 {
    let mut cost = 0.0;
    let mut coef = 1.0;
    for inst in insts {
        match inst {
            Inst::Raw(s) => {
                if s.starts_with("if") {
                    coef *= 0.5;
                } else if s.starts_with("} else") {
                    // do nothing
                } else if s.starts_with("}") {
                    coef *= 2.0;
                } else {
                    cost += coef * 2.0;
                }
            }
            Inst::Save(..) => {
                cost += coef;
            }
            Inst::Act(_, acts) => {
                cost += coef * acts.len() as f64;
            }
        }
    }
    cost
}

pub struct GephiDumpCfg {
    pub node_file: PathBuf,
    pub edge_file: PathBuf,
}

impl GephiDumpCfg {
    pub fn from_prefix(prefix: &str) -> Self {
        Self {
            node_file: PathBuf::from(format!("{}.node.csv", prefix)),
            edge_file: PathBuf::from(format!("{}.edge.csv", prefix)),
        }
    }
}

pub fn dump_logic_graph_gephi(design: &Design, cfg: &GephiDumpCfg) -> std::io::Result<()> {
    let mut node_file = BufWriter::new(File::create(&cfg.node_file)?);
    let mut edge_file = BufWriter::new(File::create(&cfg.edge_file)?);
    writeln!(node_file, "ID,Label,Order,Weight")?;
    writeln!(edge_file, "Source,Target")?;
    for logic in &design.logics {
        writeln!(
            node_file,
            "{},{},{},{}",
            logic.id.0,
            logic.id.0,
            logic.id.0,
            logic.insts.len()
        )?;
    }
    for logic in &design.logics {
        for next in &logic.next {
            writeln!(edge_file, "{},{}", logic.id.0, next.0)?;
        }
    }
    Ok(())
}

impl LogicId {
    fn to_nodeid(self) -> NodeIndex {
        node_index(self.0 as usize)
    }
}

pub fn logics_to_petgraph(design: &Design) -> DiGraph<f64, ()> {
    let mut graph = DiGraph::new();
    for logic in &design.logics {
        let cost = compute_inst_costs(&logic.insts);
        graph.add_node(cost);
    }
    for logic in &design.logics {
        for next in &logic.next {
            graph.add_edge(logic.id.to_nodeid(), next.to_nodeid(), ());
        }
    }
    graph
}

pub fn compute_depth<N, E>(g: &DiGraph<N, E>, dir: Direction) -> Vec<usize> {
    let mut dep = vec![0; g.node_count()];
    let mut update = |node: NodeIndex| {
        dep[node.index()] = g
            .neighbors_directed(node, dir)
            .map(|n| dep[n.index()] + 1)
            .max()
            .unwrap_or(0);
    };
    if matches!(dir, Direction::Incoming) {
        for node in g.node_indices() {
            update(node);
        }
    } else {
        for node in g.node_indices().rev() {
            update(node);
        }
    };
    dep
}

pub fn compute_topo_gen<N, E>(g: &DiGraph<N, E>) -> Vec<f64> {
    let dep0 = compute_depth(g, Direction::Incoming);
    let dep1 = compute_depth(g, Direction::Outgoing);
    let md = dep0.iter().max().cloned().unwrap_or(0);
    let mut bc = vec![0.0; md + 1];
    for (&d0, &d1) in dep0.iter().zip(dep1.iter()) {
        let l = d0;
        let r = md - d1 + 1;
        let w = 1.0 / (r - l) as f64;
        for i in l..r {
            bc[i] += w;
        }
    }
    bc
}

pub fn print_logic_info(design: &Design) {
    let graph = logics_to_petgraph(design);
    let avg_degree = graph.edge_count() as f64 / graph.node_count() as f64;
    let avg_cost = graph.node_weights().sum::<f64>() / graph.node_count() as f64;
    let num_sinks = graph
        .node_indices()
        .filter(|&node| graph.neighbors_directed(node, Direction::Outgoing).count() == 0)
        .count();
    let num_sources = graph
        .node_indices()
        .filter(|&node| graph.neighbors_directed(node, Direction::Incoming).count() == 0)
        .count();
    let topo_gen = compute_topo_gen(&graph);
    let mut avg_depth = 0.0;
    for (i, w) in topo_gen.iter().enumerate() {
        avg_depth += w * i as f64;
    }
    avg_depth /= topo_gen.iter().sum::<f64>();
    eprintln!("Edges:    {}", graph.edge_count());
    eprintln!("Nodes:    {}", graph.node_count());
    eprintln!("Avg deg:  {:.2}", avg_degree);
    eprintln!("Avg cst:  {:.2}", avg_cost);
    eprintln!("Sinks:    {}", num_sinks);
    eprintln!("Sources:  {}", num_sources);
    eprintln!("Topo gen: {}", topo_gen.len());
    eprintln!("Avg Depth: {:.2}", avg_depth);
    eprintln!(
        "Avg Width: {:.2}",
        graph.node_count() as f64 / num_sinks as f64
    );
    eprint!("    GEN:");
    for w in topo_gen {
        eprint!(" {:.1}", w);
    }
    eprintln!();
}
