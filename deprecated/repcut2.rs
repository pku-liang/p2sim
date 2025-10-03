use itertools::Itertools;
use ordered_float::OrderedFloat;
use petgraph::{Direction, algo::toposort, graph::DiGraph};
use priority_queue::PriorityQueue;

use crate::{analysis::compute_inst_costs, index_ext, ir::*, IndexExt};

pub fn build_logic_graph(design: &Design) -> DiGraph<(), ()> {
    let mut graph = DiGraph::new();
    for _ in &design.logics {
        graph.add_node(());
    }
    for state in &design.states {
        for read in &state.creads {
            for write in &state.writes {
                graph.add_edge(write.to_nodeid(), read.to_nodeid(), ());
            }
        }
    }
    assert!(toposort(&graph, Default::default()).is_ok());
    graph
}

pub struct UFP<T: Copy + Eq + From<usize> + Into<usize>> {
    pub parent: Vec<T>,
}

impl<T: Copy + Eq + From<usize> + Into<usize>> UFP<T> {
    pub fn new(n: usize) -> Self {
        Self {
            parent: (0..n).map(T::from).collect(),
        }
    }
    pub fn find(&self, x: T) -> T {
        let mut root = x;
        while self.parent[root.into()] != root {
            root = self.parent[root.into()];
        }
        root
    }
    pub fn find_mut(&mut self, x: T) -> T {
        let mut root = x;
        while self.parent[root.into()] != root {
            root = self.parent[root.into()];
        }
        let mut ptr = x;
        while ptr != root {
            ptr = self.parent[ptr.into()];
            self.parent[ptr.into()] = root;
        }
        root
    }
    pub fn update_all(&mut self) {
        for i in 0..self.parent.len() {
            self.find_mut(i.into());
        }
    }
    pub fn merge_to(&mut self, x: T, y: T) -> bool {
        let rx = self.find_mut(x);
        let ry = self.find_mut(y);
        if rx == ry {
            return false;
        }
        self.parent[rx.into()] = ry;
        return true;
    }
}

pub fn check_pinned(design: &Design, id: LogicId) -> bool {
    let logic = &design.logics[id];
    // foreign logic
    if logic.always {
        return true;
    }
    // large array
    for own in &logic.own {
        if design.states[*own].element_count() > 128 {
            return true;
        }
    }
    return false;
}

#[derive(Default, Clone)]
pub struct SinkView {
    nodes: Vec<LogicId>,
    edges: Vec<usize>,
    total_weight: f64,
}

impl SinkView {
    pub fn present(&self) -> bool {
        !self.nodes.is_empty()
    }
}

pub struct HGraph {
    is_sink: Vec<bool>,       // whether the node is a sink
    edges: Vec<Vec<LogicId>>, // edges of the hyper graph
    edge_weights: Vec<f64>,   // weights of the edges
    sinks: Vec<SinkView>,
}

impl HGraph {
    pub fn iou(&self, a: LogicId, b: LogicId) -> f64 {
        let mut union = 1.0;
        let mut inters = 0.0;
        let mut pa = 0;
        let mut pb = 0;
        let sa = &self.sinks[a.0 as usize];
        let sb = &self.sinks[b.0 as usize];
        while pa < sa.edges.len() && pb < sb.edges.len() {
            if sa.edges[pa] == sb.edges[pb] {
                inters += self.edge_weights[sa.edges[pa]];
                pa += 1;
                pb += 1;
            } else if sa.edges[pa] < sb.edges[pb] {
                union += self.edge_weights[sa.edges[pa]];
                pa += 1;
            } else {
                union += self.edge_weights[sb.edges[pb]];
                pb += 1;
            }
        }
        while pa < sa.edges.len() {
            union += self.edge_weights[sa.edges[pa]];
            pa += 1;
        }
        while pb < sb.edges.len() {
            union += self.edge_weights[sb.edges[pb]];
            pb += 1;
        }
        inters / union
    }
    pub fn merge_sinks(&mut self, a: LogicId, b: LogicId) -> LogicId {
        assert!(self.is_sink[a] && self.is_sink[b]);
        assert!(self.sinks[a.0 as usize].present() && self.sinks[b.0 as usize].present());
        let sink = std::mem::take(&mut self.sinks[a.0 as usize]);
        let target = &mut self.sinks[b.0 as usize];
        target.nodes.extend(sink.nodes.into_iter());
        target.nodes.sort();
        for &eid in &sink.edges {
            self.edges[eid].retain(|&node| node != a);
        }
        target.edges.extend(sink.edges.into_iter());
        target.edges.sort();
        target.edges.dedup();
        let mut total_weight = 0.0;
        for eid in target.edges.iter() {
            total_weight += self.edge_weights[*eid];
        }
        target.total_weight += total_weight;
        b
    }
    pub fn sinks(&self) -> impl Iterator<Item = LogicId> + '_ {
        self.sinks
            .iter()
            .enumerate()
            .filter_map(|(i, sink)| sink.present().then_some(i.into()))
    }
    pub fn neighbors(&self, id: LogicId) -> impl Iterator<Item = LogicId> + '_ {
        self.sinks[id.0 as usize]
            .edges
            .iter()
            .flat_map(|&eid| self.edges[eid].iter().cloned())
            .unique()
    }
}

pub fn build_hgraph(design: &Design) -> HGraph {
    let mut is_sink = vec![false; design.logics.len()];
    let mut sink_ufp = UFP::new(design.logics.len());
    let mut is_pinned = vec![false; design.logics.len()];
    let graph = build_logic_graph(design);
    for logic in &design.logics {
        is_pinned[logic.id] = check_pinned(design, logic.id);
        let has_seq_read = logic
            .own
            .iter()
            .find(|own| !design.states[**own].sreads.is_empty())
            .is_some();
        let has_no_succ = graph
            .neighbors_directed(logic.id.to_nodeid(), Direction::Outgoing)
            .count()
            == 0;
        for prev in graph.neighbors_directed(logic.id.to_nodeid(), Direction::Incoming) {
            if is_pinned[prev.index()] {
                is_pinned[logic.id] = true;
                sink_ufp.merge_to(logic.id, prev.index().into());
                break;
            }
        }
        if (has_seq_read && has_no_succ) || is_pinned[logic.id] {
            is_sink[logic.id] = true;
        }
    }
    sink_ufp.update_all();
    let mut edges: Vec<Vec<LogicId>> = vec![vec![]; design.logics.len()];
    let mut edge_weights = vec![0.0; design.logics.len()];
    for id in design.logics.ids().rev() {
        let mut cur_edges = vec![];
        if is_sink[id] {
            cur_edges.push(id);
        }
        for succ in graph.neighbors_directed(id.to_nodeid(), Direction::Outgoing) {
            cur_edges.extend(edges[succ.index()].iter().cloned());
        }
        for edge in cur_edges.iter_mut() {
            *edge = sink_ufp.parent[edge.0 as usize];
        }
        cur_edges.sort();
        cur_edges.dedup();
        edges[id] = cur_edges;
        edge_weights[id] = compute_inst_costs(&design.logics[id].insts);
    }
    let mut sinks = vec![SinkView::default(); design.logics.len()];
    for (i, &prt) in sink_ufp.parent.iter().enumerate() {
        sinks[prt].nodes.push(i.into());
    }
    for (i, edges) in edges.iter().enumerate() {
        for &id in edges {
            sinks[id].edges.push(i);
            sinks[id].total_weight += edge_weights[i];
        }
    }
    HGraph {
        is_sink,
        edges,
        edge_weights,
        sinks,
    }
}

index_ext!(containers LogicId, SinkView, f64, Vec<LogicId>);

pub fn partition(hg: &mut HGraph, threshold: f64) {
    let mut pq = PriorityQueue::new();
    for s in hg.sinks() {
        for nei in hg.neighbors(s) {
            pq.push((s, nei), OrderedFloat(hg.iou(s, nei)));
        }
    }
    while let Some(((s, nei), _w)) = pq.pop() {
        if !hg.sinks[s].present() || !hg.sinks[nei].present() {
            continue;
        }
        if hg.sinks[s].total_weight + hg.sinks[nei].total_weight > threshold {
            continue;
        }
        eprintln!("merge {:?} and {:?}, cost={}", s, nei, _w.0);
        let merged = hg.merge_sinks(s, nei);
        for nei in hg.neighbors(merged) {
            pq.change_priority(&(merged, nei), OrderedFloat(hg.iou(merged, nei)));
            pq.change_priority(&(nei, merged), OrderedFloat(hg.iou(nei, merged)));
        }
    }
}

struct DesignParter {
    visit: Vec<u32>,
    counter: u32,
    parts: Vec<LogicId>,
}

impl DesignParter {
    fn new(graph: &DiGraph<(), ()>) -> Self {
        Self {
            visit: vec![0; graph.node_count()],
            counter: 0,
            parts: vec![],
        }
    }
    fn visit(&mut self, graph: &DiGraph<(), ()>, node: LogicId) {
        self.parts.push(node);
        for prev in graph.neighbors_directed(node.to_nodeid(), Direction::Incoming) {
            let cond = self.visit[prev.index()] != self.counter;
            if cond {
                self.visit[prev.index()] = self.counter;
                self.visit(graph, prev.index().into());
            }
        }
    }
    fn find_part(&mut self, graph: &DiGraph<(), ()>, part: &[LogicId]) -> Vec<LogicId> {
        self.counter += 1;
        for node in part {
            if self.visit[node.index()] != self.counter {
                self.visit[node.index()] = self.counter;
                self.visit(graph, *node);
            }
        }
        self.parts.sort();
        std::mem::take(&mut self.parts)
    }
}

struct RepPart {
    states: Vec<State>,
    logics: Vec<Logic>,
}

pub fn partition_design(design: &Design, parts: &[Vec<LogicId>]) {
    let graph = build_logic_graph(design);
    let mut parter = DesignParter::new(&graph);
    for part in parts {
        let parts = parter.find_part(&graph, part);
    }
}