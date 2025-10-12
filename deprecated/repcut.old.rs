use std::io::Write;

use itertools::Itertools;
use log::{debug, info, warn};
use ordered_float::OrderedFloat;
use petgraph::{
    Direction,
    algo::tred,
    graph::{DiGraph, node_index},
    visit::IntoNeighbors,
};
use priority_queue::PriorityQueue;
use rayon::iter::{IntoParallelIterator, ParallelBridge, ParallelIterator};
use rustc_hash::FxHashMap;

use crate::{IndexExt, ir::*};

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

struct HGraphData {
    num_nodes: usize,
    node_rev: Vec<Ix>,
    indptr: Vec<usize>,
    indices: Vec<usize>,
}

impl HGraph {
    fn merge_part(&mut self, part: &[Ix]) {
        if part.len() <= 1 {
            return;
        }
        let mut iter = part.into_iter();
        let &cur = iter.next().unwrap();
        for &next in iter {
            self.modify_node(next, cur);
        }
        self.update(cur);
    }
    #[allow(unused)]
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
            // self.succs[pred as usize].sort();
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
    fn valid_sink_ids(&self) -> impl Iterator<Item = Ix> + '_ {
        self.sinks
            .iter()
            .enumerate()
            .filter_map(|(i, sink)| sink.present().then_some(i as Ix))
    }
    fn valid_sinks(&self) -> impl Iterator<Item = (Ix, &SinkView)> + '_ {
        self.sinks
            .iter()
            .enumerate()
            .filter_map(|(i, sink)| sink.present().then_some((i as Ix, sink)))
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
    fn get_succ_set(&self, iter: impl IntoIterator<Item = Ix>) -> Vec<Ix> {
        iter.into_iter()
            .flat_map(|succ| self.succs[succ as usize].iter().cloned())
            .sorted()
            .dedup()
            .collect()
    }
    fn to_data(&self) -> HGraphData {
        let sink_ids: FxHashMap<_, _> = self
            .valid_sink_ids()
            .enumerate()
            .map(|(i, id)| (id, i))
            .collect();
        let sink_rev_map = self.valid_sink_ids().collect::<Vec<_>>();
        let mut indptr = vec![0];
        let mut indices = vec![];
        for succ in &self.succs {
            indptr.push(indptr.last().unwrap() + succ.len());
            for &succ in succ {
                indices.push(sink_ids[&succ]);
            }
        }
        HGraphData {
            num_nodes: sink_rev_map.len(),
            node_rev: sink_rev_map,
            indptr,
            indices,
        }
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
        if design.states[*own].element_count() > 8192 {
            return true;
        }
    }
    return false;
}

fn build_dep_info(design: &Design) -> DepInfo {
    info!("Start building dep info");
    info!("1. Build logic graph");
    let mut graph = DiGraph::new();
    for _ in &design.logics {
        graph.add_node(());
    }
    info!("2. Compute is_sink and is_pinned");
    let mut is_sink = vec![false; design.logics.len()];
    let mut is_pinned = vec![false; design.logics.len()];
    for state in &design.states {
        if state.is_seq {
            if state.modifies.len() > 1 {
                warn!("seq state {} with multiple modifies", state.name);
            }
            for read in &state.uses {
                for write in &state.modifies {
                    if graph.contains_edge(read.to_nodeid(), write.to_nodeid()) {
                        continue;
                    }
                    graph.add_edge(read.to_nodeid(), write.to_nodeid(), ());
                }
            }
            for write in &state.modifies {
                is_sink[write.0 as usize] = true;
                // if state.element_count() > 8192 {
                //     is_pinned[write.0 as usize] = true;
                // }
            }
        } else {
            for read in &state.uses {
                for write in &state.modifies {
                    if graph.contains_edge(write.to_nodeid(), read.to_nodeid()) {
                        continue;
                    }
                    graph.add_edge(write.to_nodeid(), read.to_nodeid(), ());
                }
            }
        }
    }
    info!(
        "  sinks={}, pinned={}",
        is_sink.iter().filter(|&&b| b).count(),
        is_pinned.iter().filter(|&&b| b).count()
    );
    for logic in &design.logics {
        if logic.always {
            is_pinned[logic.id] = true;
        }
        let has_no_succ = graph
            .neighbors_directed(logic.id.to_nodeid(), Direction::Outgoing)
            .count()
            == 0;
        if has_no_succ {
            is_sink[logic.id] = true;
        }
    }
    info!(
        "  sinks={}, pinned={}",
        is_sink.iter().filter(|&&b| b).count(),
        is_pinned.iter().filter(|&&b| b).count()
    );
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
    let hg = HGraph { sinks, succs };
    // info!("3. Merge pinned nodes");
    // for i in 0..depinfo.node_count() {
    //     if depinfo.is_pinned[i] {
    //         hg.merge_row(i as Ix);
    //     }
    // }
    info!("4. Verify graph");
    // hg.verify_all();
    hg
}

fn compute_choices(hg: &HGraph) -> Vec<((Ix, Ix), f64)> {
    let sinks = hg.valid_sink_ids().collect::<Vec<_>>();
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

pub struct UFP {
    pub parent: Vec<usize>,
}

impl UFP {
    pub fn new(n: usize) -> Self {
        Self {
            parent: (0..n).collect(),
        }
    }
    pub fn find(&self, x: usize) -> usize {
        let mut root = x;
        while self.parent[root] != root {
            root = self.parent[root];
        }
        root
    }
    pub fn find_mut(&mut self, x: usize) -> usize {
        let mut root = x;
        while self.parent[root] != root {
            root = self.parent[root];
        }
        let mut ptr = x;
        while ptr != root {
            ptr = self.parent[ptr];
            self.parent[ptr] = root;
        }
        root
    }
    pub fn update_all(&mut self) {
        for i in 0..self.parent.len() {
            self.find_mut(i);
        }
    }
    pub fn merge_to(&mut self, x: usize, y: usize) -> bool {
        let rx = self.find_mut(x);
        let ry = self.find_mut(y);
        if rx == ry {
            return false;
        }
        self.parent[rx] = ry;
        return true;
    }
}

fn partition(hg: &mut HGraph, threshold: f64) {
    info!("Start partitioning");
    info!("1. Populate choices to priority queue");
    let mut pq = PriorityQueue::new();
    let choices = compute_choices(hg);
    for ((s, nei), _w) in choices {
        if s < nei {
            pq.push((s, nei), OrderedFloat(_w));
        }
    }
    info!("2. Fast sweep");
    let mut ufp = UFP::new(hg.sinks.len());
    while let Some(((s, nei), _w)) = pq.pop() {
        if _w.0 < 0.9 {
            break;
        }
        ufp.merge_to(s as usize, nei as usize);
    }
    ufp.update_all();
    let mut parts = vec![vec![]; ufp.parent.len()];
    for (i, p) in ufp.parent.iter().enumerate() {
        parts[*p].push(i as Ix);
    }
    for part in parts {
        if part.len() > 1 {
            hg.merge_part(&part);
        }
    }
    info!("2. Partitioning");
    let mut num_merged = 0;
    let num_sinks = hg.valid_sink_ids().count();
    while let Some(((s, nei), _w)) = pq.pop() {
        if !hg.sinks[s as usize].present() || !hg.sinks[nei as usize].present() {
            continue;
        }
        // let unioned = (hg.sinks[s as usize].preds.len() + hg.sinks[nei as usize].preds.len())
        //     as f64
        //     / (_w.0 + 1.0);
        // if unioned > threshold {
        //     continue;
        // }
        let unioned = hg.sinks[s as usize].nodes.len() + hg.sinks[nei as usize].nodes.len();
        if unioned as f64 > threshold {
            continue;
        }
        num_merged += 1;
        if num_merged % 200 == 0 {
            info!(
                "  Merged {} / {} ({:.2}%) sinks, iou={:.5}, union={:5}",
                num_merged,
                num_sinks,
                num_merged as f64 / num_sinks as f64 * 100.0,
                _w.0,
                unioned
            );
        }
        debug!(
            "  merge {:?} and {:?}, iou={}, unioned={}",
            s, nei, _w.0, unioned
        );
        let merged = hg.merge_col(s, nei);
        assert!(hg.sinks[merged as usize].present());
        for nei in hg.sink_neighbors(merged) {
            if merged < nei {
                pq.push((merged, nei), OrderedFloat(hg.iou_col(merged, nei)));
            }
            if nei < merged {
                pq.push((nei, merged), OrderedFloat(hg.iou_col(nei, merged)));
            }
        }
    }
    info!("3. Finish, num_sinks={}", hg.valid_sink_ids().count());
    let sink_sizes = hg
        .valid_sink_ids()
        .map(|s| hg.sinks[s as usize].preds.len())
        .collect_vec();
    let total_size = sink_sizes.iter().sum::<usize>();
    info!("  Sink sizes: {:?}", sink_sizes);
    info!("  Size: {} => {}", hg.succs.len(), total_size);
}

#[derive(Default, Clone)]
struct SinkInfo {
    seq_reads: usize,
    seq_writes: usize,
    seq_local: usize,
    comb_states: usize,
}

fn print_partition_stats(design: &Design, hg: &HGraph) {
    let mut sink_info = vec![SinkInfo::default(); hg.sinks.len()];
    let mut sink_map = FxHashMap::default();
    for (id, sink) in hg.valid_sinks() {
        for node in sink.nodes.iter() {
            sink_map.insert(*node, id);
        }
    }
    for (_id, state) in design.states.enumerate_ids() {
        if state.is_seq {
            let belong = state
                .modifies
                .iter()
                .map(|&m| sink_map.get(&m.0).expect("modify not in sink map").clone())
                .sorted()
                .dedup()
                .collect_vec();
            if belong.len() > 1 {
                warn!("seq state {} with multiple modifies", state.name);
            }
            for &b in &belong {
                sink_info[b as usize].seq_writes += 1;
            }
            let reads_set = hg.get_succ_set(state.uses.iter().map(|&read| read.0));
            for &r in &reads_set {
                sink_info[r as usize].seq_reads += 1;
            }
            match (&belong[..], &reads_set[..]) {
                ([b], [r]) => {
                    if b == r {
                        sink_info[*b as usize].seq_local += 1;
                    }
                }
                _ => {}
            }
        } else {
            let writes_set = hg.get_succ_set(state.modifies.iter().map(|&write| write.0));
            for &w in &writes_set {
                sink_info[w as usize].comb_states += 1;
            }
        }
    }
    eprintln!("Sink info: edges={}", hg.succs.len());
    for id in hg.valid_sink_ids() {
        eprintln!(
            "  Sink {:5}: S={:5} N={:5} SR={:5} SW={:5} SL={:5} CS={:5}",
            id,
            hg.sinks[id as usize].nodes.len(),
            hg.sinks[id as usize].preds.len(),
            sink_info[id as usize].seq_reads,
            sink_info[id as usize].seq_writes,
            sink_info[id as usize].seq_local,
            sink_info[id as usize].comb_states,
        );
    }
}

fn partition_kahypar(hg: &mut HGraph, k: usize) {
    use mt_kahypar::*;
    info!("Start partitioning with kahypar");
    info!("1. Build context");
    let ctx = Context::builder()
        .preset(Preset::Deterministic)
        .k(k as i32)
        .epsilon(0.03)
        .objective(Objective::Km1)
        .seed(42)
        .verbose(false)
        .build()
        .unwrap();
    info!("2. Build hypergraph");
    let hg_data = hg.to_data();
    info!(
        "  num_nodes={}, num_edges={}",
        hg_data.num_nodes,
        hg_data.indptr.len()
    );
    let ka_hg = Hypergraph::from_adjacency(
        &ctx,
        hg_data.num_nodes,
        &hg_data.indptr,
        &hg_data.indices,
        None,
        None,
    )
    .unwrap();
    info!("3. Partition");
    let part = ka_hg.partition().unwrap();
    info!(
        "  cut={}, km1={}, imbalance={}, soed={}",
        part.cut(),
        part.km1(),
        part.imbalance(),
        part.soed()
    );
    info!("4. Extract partition");
    let partition = part.extract_partition();
    assert!(partition.len() == hg_data.num_nodes);
    assert!(hg_data.node_rev.len() == hg_data.num_nodes);
    let mut part_rev: FxHashMap<i32, Vec<Ix>> = FxHashMap::default();
    for (i, p) in partition.iter().enumerate() {
        part_rev.entry(*p).or_default().push(i as Ix);
    }
    let parts = part_rev.into_iter().sorted().collect::<Vec<_>>();
    for (id, part) in parts {
        info!("  Merge part {:5}: {}", id, part.len());
        hg.merge_part(
            &part
                .iter()
                .map(|&i| hg_data.node_rev[i as usize])
                .collect_vec(),
        );
    }
    info!("5. Finish");
}

pub fn repcut(design: &Design) {
    let depinfo = build_dep_info(design);
    let mut hg = build_hgraph(&depinfo);
    // partition(&mut hg, 1000.0);
    partition_kahypar(&mut hg, 8);
    print_partition_stats(design, &hg);
}
