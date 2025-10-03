use itertools::Itertools;
use log::{debug, info};
use ordered_float::OrderedFloat;
use petgraph::{
    Direction,
    algo::tred,
    graph::{DiGraph, node_index},
    visit::IntoNeighbors,
};
use priority_queue::PriorityQueue;
use rayon::iter::{IntoParallelIterator, ParallelBridge, ParallelIterator};

use crate::ir::*;

type Ix = u32;

#[derive(Default, Clone, Debug)]
struct SinkView {
    nodes: Vec<Ix>,
    preds: Vec<Ix>,
}

impl SinkView {
    fn present(&self) -> bool {
        !self.nodes.is_empty()
    }
}

struct HGraph {
    sinks: Vec<SinkView>,
    succs: Vec<Vec<Ix>>,
}

impl HGraph {
    fn merge_row(&mut self, a: Ix) {
        debug!("merge row {}", a);
        let indices = self.succs[a as usize].clone();
        if indices.is_empty() {
            return;
        }
        for succ in &indices {
            assert!(self.sinks[*succ as usize].present());
        }
        let mut iter = indices.into_iter();
        let cur = iter.next().unwrap();
        for next in iter {
            self.modify_node(next, cur);
            // cur = self.merge_col(cur, next);
        }
        self.update(cur);
    }
    #[allow(unused)]
    fn verify_all(&self) {
        for i in 0..self.succs.len() {
            for succ in &self.succs[i] {
                assert!(
                    self.sinks[*succ as usize].present(),
                    "sink {} is not present",
                    succ
                );
                assert!(
                    self.sinks[*succ as usize].preds.contains(&(i as Ix)),
                    "sink {} does not contain {}, succs[{}] = {:?}, sinks[{}] = {:?}",
                    succ,
                    i,
                    i,
                    self.succs[i],
                    i,
                    self.sinks[i]
                );
            }
        }
        for i in 0..self.sinks.len() {
            for pred in &self.sinks[i].preds {
                assert!(self.succs[*pred as usize].contains(&(i as Ix)));
            }
        }
    }
    fn modify_node(&mut self, a: Ix, b: Ix) {
        let source = std::mem::take(&mut self.sinks[a as usize]);
        let target = &mut self.sinks[b as usize];
        for &pred in source.preds.iter() {
            self.succs[pred as usize].retain(|&succ| succ != a && succ != b);
            self.succs[pred as usize].push(b);
            self.succs[pred as usize].sort();
        }
        target.nodes.extend(source.nodes.into_iter());
        target.preds.extend(source.preds.into_iter());
    }
    fn merge_col(&mut self, mut a: Ix, mut b: Ix) -> Ix {
        assert!(a != b);
        assert!(self.sinks[a as usize].present());
        assert!(self.sinks[b as usize].present());
        if self.sinks[a as usize].preds.len() > self.sinks[b as usize].preds.len() {
            std::mem::swap(&mut a, &mut b);
        }
        self.modify_node(a, b);
        self.update(b);
        b
    }
    fn sink_neighbors(&self, a: Ix) -> impl Iterator<Item = Ix> + '_ {
        assert!(self.sinks[a as usize].present());
        self.sinks[a as usize]
            .preds
            .iter()
            .flat_map(move |&pred| self.succs[pred as usize].iter())
            .cloned()
            .sorted()
            .dedup()
    }
    fn valid_sinks(&self) -> impl Iterator<Item = Ix> + '_ {
        self.sinks
            .iter()
            .enumerate()
            .filter_map(|(i, sink)| sink.present().then_some(i as Ix))
    }
    fn update(&mut self, u: Ix) {
        assert!(self.sinks[u as usize].present());
        let sink = &mut self.sinks[u as usize];
        sink.nodes.sort();
        sink.preds.sort();
        sink.preds.dedup();
    }
    fn iou_col(&self, a: Ix, b: Ix) -> f64 {
        let mut union_size = 0;
        let mut inters_size = 0;
        let mut a_iter = 0;
        let mut b_iter = 0;
        let a_list = &self.sinks[a as usize].preds;
        let b_list = &self.sinks[b as usize].preds;
        while a_iter < a_list.len() && b_iter < b_list.len() {
            if a_list[a_iter] == b_list[b_iter] {
                inters_size += 1;
                union_size += 1;
                a_iter += 1;
                b_iter += 1;
            } else if a_list[a_iter] < b_list[b_iter] {
                union_size += 1;
                a_iter += 1;
            } else {
                union_size += 1;
                b_iter += 1;
            }
        }
        while a_iter < a_list.len() {
            union_size += 1;
            a_iter += 1;
        }
        while b_iter < b_list.len() {
            union_size += 1;
            b_iter += 1;
        }
        inters_size as f64 / union_size as f64
    }
}

struct DepInfo {
    graph: DiGraph<(), ()>,
    is_pinned: Vec<bool>,
    is_sink: Vec<bool>,
}

impl DepInfo {
    fn node_count(&self) -> usize {
        self.graph.node_count()
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

fn build_dep_info(design: &Design) -> DepInfo {
    let mut graph = DiGraph::new();
    for _ in &design.logics {
        graph.add_node(());
    }
    for state in &design.states {
        for read in &state.creads {
            for write in &state.writes {
                if graph.contains_edge(write.to_nodeid(), read.to_nodeid()) {
                    continue;
                }
                graph.add_edge(write.to_nodeid(), read.to_nodeid(), ());
            }
        }
    }
    let mut is_sink = vec![false; design.logics.len()];
    let mut is_pinned = vec![false; design.logics.len()];
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
        is_sink[logic.id] = has_seq_read || has_no_succ;
    }
    DepInfo {
        graph,
        is_pinned,
        is_sink,
    }
}

fn build_hgraph(depinfo: &DepInfo) -> HGraph {
    info!("Start building hgraph");
    info!("1. Compute toposort and transitive closure");
    let toposort = (0..depinfo.node_count())
        .map(node_index)
        .collect::<Vec<_>>();
    let (succs, _) = tred::dag_to_toposorted_adjacency_list::<_, Ix>(&depinfo.graph, &toposort);
    let (_, tclos) = tred::dag_transitive_reduction_closure::<_, Ix>(&succs);
    let mut sinks = vec![SinkView::default(); depinfo.node_count()];
    let mut succs = vec![vec![]; depinfo.node_count()];
    info!("2. Build graph structure");
    for i in 0..depinfo.node_count() {
        if depinfo.is_sink[i] {
            sinks[i].nodes.push(i as Ix);
        }
    }
    for i in 0..depinfo.node_count() {
        let create_iter = || {
            tclos
                .neighbors(i as Ix)
                .filter(|&succ| depinfo.is_sink[succ as usize])
        };
        let count = create_iter().count();
        succs[i].reserve(count + 10);
        succs[i].extend(create_iter());
        if depinfo.is_sink[i] {
            succs[i].push(i as Ix);
        }
        succs[i].sort();
        assert!(!succs[i].is_empty());
        for &it in &succs[i] {
            assert!(sinks[it as usize].present());
            sinks[it as usize].preds.push(i as Ix);
        }
    }
    let mut hg = HGraph { sinks, succs };
    info!("3. Merge pinned nodes");
    for i in 0..depinfo.node_count() {
        if depinfo.is_pinned[i] {
            hg.merge_row(i as Ix);
        }
    }
    hg.verify_all();
    hg
}

fn compute_choices(hg: &HGraph) -> Vec<((Ix, Ix), f64)> {
    let sinks = hg.valid_sinks().collect::<Vec<_>>();
    sinks
        .into_par_iter()
        .flat_map(|s| compute_new_choices(hg, s))
        .collect()
}

fn compute_new_choices(
    hg: &HGraph,
    s: Ix,
) -> impl rayon::iter::ParallelIterator<Item = ((Ix, Ix), f64)> + '_ {
    hg.sink_neighbors(s)
        .par_bridge()
        .map(move |nei| ((s, nei), hg.iou_col(s, nei)))
}

fn partition(hg: &mut HGraph, threshold: f64) {
    info!("Start partitioning");
    info!("1. Populate choices to priority queue");
    let mut pq = PriorityQueue::new();
    let choices = compute_choices(hg);
    for ((s, nei), _w) in choices {
        if s < nei {
            pq.push((s, nei), OrderedFloat(hg.iou_col(s, nei)));
        }
    }
    info!("2. Partitioning");
    let mut num_merged = 0;
    let num_sinks = hg.valid_sinks().count();
    while let Some(((s, nei), _w)) = pq.pop() {
        if !hg.sinks[s as usize].present() || !hg.sinks[nei as usize].present() {
            continue;
        }
        let unioned = (hg.sinks[s as usize].preds.len() + hg.sinks[nei as usize].preds.len())
            as f64
            / (_w.0 + 1.0);
        if unioned > threshold {
            continue;
        }
        num_merged += 1;
        if num_merged % 200 == 0 {
            info!(
                "  Merged {} / {} ({:.2}%) sinks",
                num_merged,
                num_sinks,
                num_merged as f64 / num_sinks as f64 * 100.0
            );
        }
        debug!(
            "  merge {:?} and {:?}, iou={}, unioned={}",
            s, nei, _w.0, unioned
        );
        let merged = hg.merge_col(s, nei);
        assert!(hg.sinks[merged as usize].present());
        for nei in hg.sink_neighbors(merged) {
            pq.change_priority(&(merged, nei), OrderedFloat(hg.iou_col(merged, nei)));
            pq.change_priority(&(nei, merged), OrderedFloat(hg.iou_col(nei, merged)));
        }
    }
    info!("3. Finish, num_sinks={}", hg.valid_sinks().count());
    let sink_sizes = hg
        .valid_sinks()
        .map(|s| hg.sinks[s as usize].nodes.len())
        .collect_vec();
    info!("  Sink sizes: {:?}", sink_sizes);
}

pub fn repcut(design: &Design) {
    let depinfo = build_dep_info(design);
    let mut hg = build_hgraph(&depinfo);
    partition(&mut hg, 1000.0);
}

