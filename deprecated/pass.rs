use std::collections::BTreeSet;

use index_vec_ext::{IndexExt, index_ext};
use itertools::Itertools;
use log::{info, warn};
use petgraph::{
    algo::{toposort, tred}, graph::{node_index, DiGraph, NodeIndex}, visit::IntoNeighbors, Direction
};
use rustc_hash::{FxHashMap, FxHashSet};
use mt_kahypar::{Context, Objective, Preset, Hypergraph};
use tabular::{Row, Table};

use crate::{act::ActInfo, ir::*, sch::{linear_schedule, LinearSchOptions, Perm, SchProblem}};

pub type ActMap = FxHashMap<StateId, Vec<BlockId>>;

pub fn get_act_map(design: &Design) -> ActMap {
    let mut act_map: ActMap = FxHashMap::default();
    for (blk_id, block) in design.blocks.enumerate_ids() {
        for read in &block.reads {
            act_map.entry(*read).or_default().push(blk_id);
        }
    }
    act_map
}

pub fn verify_act_map(design: &Design) {
    let act_map = get_act_map(design);
    for block in &design.blocks {
        for inst in &block.insts {
            if let Inst::WritePost { sid, acts, .. } = inst {
                let act_map_acts = act_map.get(sid).map(|x| &x[..]).unwrap_or(&[]);
                for a in acts {
                    assert!(
                        act_map_acts.contains(a),
                        "Act {} doesn't include {:?}, ({:?})",
                        design.states[*sid].name,
                        a,
                        act_map_acts
                    );
                }
                if acts.len() < act_map_acts.len() {
                    warn!(
                        "Activates more blocks than gsim: {} {} => {}",
                        design.states[*sid].name,
                        acts.len(),
                        act_map_acts.len()
                    );
                }
            }
        }
    }
}

pub fn verify_never_act(design: &Design) {
    let mut all_acts = FxHashSet::default();
    for block in &design.blocks {
        for inst in &block.insts {
            if let Inst::WritePost { acts, .. } = inst {
                for a in acts {
                    all_acts.insert(*a);
                }
            }
        }
    }
    for reset in &design.resets {
        for act in &reset.acts {
            all_acts.insert(*act);
        }
    }
    let mut never_act = vec![];
    for (blk_id, block) in design.blocks.enumerate_ids() {
        if !block.always && !all_acts.contains(&blk_id) {
            never_act.push(blk_id.0);
        }
    }
    warn!("Never act blocks: {:?}", never_act);
}

#[derive(Default, Clone, Debug)]
pub struct RWEntry {
    pub reads: Vec<BlockId>,
    pub writes: Vec<BlockId>,
}

impl RWEntry {
    pub fn is_disjoint(&self) -> bool {
        self.is_comb() || self.is_seq()
    }
    pub fn is_comb(&self) -> bool {
        if self.reads.is_empty() || self.writes.is_empty() {
            true
        } else {
            self.writes.last().unwrap() < self.reads.first().unwrap()
        }
    }
    pub fn is_seq(&self) -> bool {
        if self.reads.is_empty() || self.writes.is_empty() {
            false
        } else {
            self.reads.last().unwrap() <= self.writes.first().unwrap()
        }
    }
    pub fn iter_rw(&self) -> impl Iterator<Item = (BlockId, BlockId)> + '_ {
        self.reads
            .iter()
            .flat_map(|r| self.writes.iter().map(move |w| (*r, *w)))
    }
    pub fn iter_wuw(&self) -> impl Iterator<Item = (BlockId, BlockId)> + '_ {
        let mut i = 0;
        std::iter::from_fn(move || {
            if i + 1 >= self.writes.len() {
                None
            } else {
                let w1 = self.writes[i];
                let w2 = self.writes[i + 1];
                i += 1;
                Some((w1, w2))
            }
        })
    }
}

pub struct DepInfo {
    pub rw: Vec<RWEntry>,
    pub cdep: DiGraph<(), ()>,
    pub sdep: DiGraph<(), ()>,
}

impl DepInfo {
    pub fn is_sink(&self, bid: BlockId) -> bool {
        let seq_succs = self
            .sdep
            .neighbors_directed(bid.node_index(), Direction::Outgoing)
            .count();
        let comb_succs = self
            .cdep
            .neighbors_directed(bid.node_index(), Direction::Outgoing)
            .count();
        // if it has seq successor, it is a sink
        // but if it has no comb successor, such as a assert statement, it is also a sink
        seq_succs > 0 || comb_succs == 0
    }
}

index_ext! {containers StateId, RWEntry}

pub trait NodeIdxExt {
    fn node_index(self) -> NodeIndex;
}
impl NodeIdxExt for BlockId {
    fn node_index(self) -> NodeIndex {
        node_index(self.0 as usize)
    }
}

pub fn get_dep_info(design: &Design) -> DepInfo {
    let mut entries = vec![RWEntry::default(); design.states.len()];
    for (blk_id, block) in design.blocks.enumerate_ids() {
        for read in &block.reads {
            entries[*read].reads.push(blk_id);
        }
        for write in &block.writes {
            entries[*write].writes.push(blk_id);
        }
    }
    let mut cdep = DiGraph::new();
    let mut sdep = DiGraph::new();
    for _ in &design.blocks {
        cdep.add_node(());
        sdep.add_node(());
    }
    for (blk_id, blk) in design.blocks.enumerate_ids() {
        for next in blk.next.iter() {
            if ! cdep.contains_edge(blk_id.node_index(), next.node_index()) {
                cdep.add_edge(blk_id.node_index(), next.node_index(), ());
            }
        }
    }
    for (sid, entry) in entries.enumerate_ids() {
        if entry.is_comb() {
            for (r, w) in entry.iter_rw() {
                // write => read
                cdep.add_edge(w.node_index(), r.node_index(), ());
            }
        }
        if entry.is_seq() {
            for (r, w) in entry.iter_rw() {
                if r != w {
                    cdep.add_edge(r.node_index(), w.node_index(), ());
                }
                sdep.add_edge(w.node_index(), r.node_index(), ());
            }
        }
        for (w1, w2) in entry.iter_wuw() {
            cdep.add_edge(w1.node_index(), w2.node_index(), ());
        }
    }
    DepInfo {
        rw: entries,
        cdep,
        sdep,
    }
}

pub fn verify_dep_info(design: &Design) {
    let dep = get_dep_info(design);
    let mut large_array = vec![];
    for (sid, entry) in dep.rw.enumerate_ids() {
        if entry.writes.len() > 1 {
            warn!(
                "State has multiple writes, id={:?}, state={:?}, cnt={}",
                sid,
                design.states[sid].name,
                entry.writes.len()
            );
        }
        if design.states[sid].element_count() > 32 {
            large_array.push(design.states[sid].element_count());
        }
        assert!(
            entry.is_disjoint(),
            "state {:?} {:?} is not disjoint {:?}",
            sid,
            design.states[sid],
            entry
        );
    }
    for (blk_id, blk) in design.blocks.enumerate_ids() {
        for next in blk.next.iter() {
            if ! dep.cdep.contains_edge(blk_id.node_index(), next.node_index()) {
                warn!("block {:?} next {:?} is not in cdep", blk_id, next);
            }
            // assert!(
            //     dep.cdep.contains_edge(blk_id.node_index(), next.node_index()),
            //     "block {:?} next {:?} is not in cdep",
            //     blk_id,
            //     next
            // );
        }
    }
    large_array.sort();
    info!("Large Arrays: {:?}", large_array);
    toposort(&dep.cdep, Default::default()).expect("cycle in cdep");
}

pub fn reorder_blocks(design: &mut Design, act: &ActInfo) -> Perm {
    let dep = get_dep_info(design);
    let edges = dep.cdep.raw_edges();
    let g: DiGraph<(), i64> = dep.cdep.map(|_, n | *n, |eid, _| {
        let src = edges[eid.index()].source();
        let dst = edges[eid.index()].target();
        ActInfo::HASH_WIDTH as i64 - act.query_similarity_nodiv(src.index(), dst.index()) as i64
    });
    let prob = SchProblem {
        graph: &g,
        edge_weight_div: ActInfo::HASH_WIDTH as i64,
    };
    let opt = LinearSchOptions {
        prev_factor: num::Rational64::new(1, 10),
        max_iter: 10,
        verbose: true,
    };
    let perm = linear_schedule(prob, &opt);
    assert!(perm.ord.len() == perm.ord.iter().sorted().dedup().count());
    let mut new_blocks = Vec::with_capacity(design.blocks.len());
    for (p, &i) in perm.ord.iter().enumerate() {
        let id = node_index(i);
        for v in g.neighbors_directed(id, Direction::Outgoing) {
            assert!(perm.rnk[v.index()] > p);
        }
    }
    for i in 0..design.blocks.len() {
        let block = std::mem::take(&mut design.blocks[perm.ord[i]]);
        new_blocks.push(block);
    }
    design.blocks = new_blocks;
    prob.print_stat(&perm);
    perm
}

struct HPG {
    num_nodes: usize,
    indices: Vec<usize>,
    edges: Vec<usize>,
    edge_weights: Vec<i32>,
    node_weights: Vec<i32>,
}

impl HPG {
    fn new(node_weights: Vec<i32>) -> Self {
        Self {
            num_nodes: node_weights.len(),
            indices: vec![0],
            edges: vec![],
            edge_weights: vec![],
            node_weights,
        }
    }
    fn add_edge(&mut self, weight: i32, edges: impl IntoIterator<Item = impl Clone + Into<usize>>) {
        for e in edges {
            self.edges.push(e.clone().into());
        }
        let idx = self.edges.len();
        self.indices.push(idx);
        self.edge_weights.push(weight);
    }
    fn build<'ctx>(
        &'ctx self,
        ctx: &'ctx Context,
    ) -> Result<Hypergraph<'ctx>, mt_kahypar::Error> {
        Hypergraph::from_adjacency(
            ctx,
            self.num_nodes,
            &self.indices,
            &self.edges,
            Some(&self.edge_weights),
            Some(&self.node_weights),
        )
    }
}

#[derive(Default, Clone)]
pub struct Part {
    pub seq_reads: BTreeSet<StateId>,
    pub seq_writes: BTreeSet<StateId>,
    pub comb_states: BTreeSet<StateId>,
    pub resets: Vec<Reset>,
    pub blocks: Vec<Block>,
}

pub struct PartDesign {
    pub max_concat: u32,
    pub ext_fns: Vec<String>,
    pub states: Vec<State>,
    pub parts: Vec<Part>,
}

index_ext! {pub struct PartId(pub u32) for Part}

// 图分割时，分割的实际是状态，而非逻辑
// 1. 状态数量太多，用逻辑表示它所有的时序状态
// 2. 边表示将一个状态分给两个线程的代价
// 考虑几种情况
// 1. 大数组，不适合分割 => 加一条权重非常大的边
// 2. 多个写，放一个线程里 => 添加 wuw 依赖，w 不会有其它 read，只能放一个线程里
pub fn partition(design: &Design) -> PartDesign {
    info!("Start repcut partition");

    info!("1. Build transitive closure");
    let dep = get_dep_info(design);
    let toposort = (0..design.blocks.len()).map(node_index).collect_vec();
    let (succs, _) = tred::dag_to_toposorted_adjacency_list::<_, u32>(&dep.cdep, &toposort);
    let (_, tclos) = tred::dag_transitive_reduction_closure::<_, u32>(&succs);

    info!("2. Find sink nodes");
    let mut sinks = vec![];
    let mut sink_size = vec![];
    let mut sink_map = vec![None; design.blocks.len()];
    for bid in design.blocks.ids() {
        if dep.is_sink(bid) {
            let sink_id = sinks.len();
            sinks.push(bid);
            let mut cur_sink_size = 0;
            for write in design.blocks[bid].writes.iter() {
                if dep.rw[*write].is_seq() {
                    cur_sink_size += 1;
                }
            }
            sink_size.push(cur_sink_size as i32);
            sink_map[bid.0 as usize] = Some(sink_id);
        }
    }

    info!("3. Build Hypergraph");
    let mut builder = HPG::new(sink_size);
    let get_edges = |blk_id: BlockId| {
        tclos
            .neighbors(blk_id.0)
            .filter_map(|x| sink_map[x as usize])
    };
    for (blk_id, block) in design.blocks.enumerate_ids() {
        let weight = block.insts.len() as i32 / 32 + 1;
        builder.add_edge(weight, get_edges(blk_id));
    }
    for (sid, rw) in dep.rw.enumerate_ids() {
        if design.states[sid].element_count() > 32 {
            let weight = rw.writes.len() as i32 / 32 + 1;
            let mut edges = vec![];
            for r_blk_id in rw.reads.iter() {
                edges.extend(get_edges(*r_blk_id));
            }
            for w_blk_id in rw.writes.iter() {
                edges.extend(get_edges(*w_blk_id));
            }
            builder.add_edge(weight, edges);
        }
    }

    let k: usize = 8;

    info!("4. Partition");
    let ctx = Context::builder()
        .preset(Preset::Deterministic)
        .k(k as i32)
        .epsilon(0.03)
        .objective(Objective::Km1)
        .seed(42)
        .verbose(false)
        .build()
        .unwrap();
    let hpg = builder.build(&ctx).unwrap();

    info!("5. Extract partition");
    let part = hpg.partition().unwrap();
    let partition = part.extract_partition();
    assert!(partition.len() == sinks.len());

    let mut part_sinks = vec![vec![]; k];
    let mut inside_part = vec![BTreeSet::default(); design.blocks.len()];
    for i in design.blocks.ids() {
        if let Some(node_id) = sink_map[i.0 as usize] {
            let part_id = partition[node_id] as usize;
            inside_part[i.0 as usize].insert(part_id);
            part_sinks[part_id].push(i);
        }
    }
    for i in design.blocks.ids().rev() {
        let mut insides = vec![];
        let succ_iter = dep
            .cdep
            .neighbors_directed(i.node_index(), Direction::Outgoing);
        for j in succ_iter {
            insides.extend(inside_part[j.index()].iter().cloned());
        }
        inside_part[i.0 as usize].extend(insides);
    }

    let mut parts = vec![Part::default(); k];
    for (i, block) in design.blocks.enumerate_ids() {
        for part_id in inside_part[i.0 as usize].iter() {
            parts[*part_id].blocks.push(block.clone());
        }
    }
    for reset in design.resets.iter() {
        let mut need_add_reset = FxHashSet::default();
        for act in reset.acts.iter() {
            need_add_reset.extend(inside_part[act.0 as usize].iter().cloned());
        }
        for part_id in need_add_reset {
            parts[part_id].resets.push(reset.clone());
        }
    }
    for (part_id, part) in parts.iter_mut().enumerate() {
        for sink in part_sinks[part_id].iter() {
            for write in design.blocks[*sink].writes.iter() {
                if dep.rw[*write].is_seq() {
                    part.seq_writes.insert(*write);
                }
            }
        }
        eprintln!("part: {} sinks: {}", part_id, part_sinks[part_id].len());
        let mut writes = BTreeSet::default();
        for block in part.blocks.iter() {
            for read in block.reads.iter() {
                if dep.rw[*read].is_seq() {
                    part.seq_reads.insert(*read);
                } else if dep.rw[*read].is_comb() {
                    part.comb_states.insert(*read);
                } else {
                    panic!("read {:?} is not seq or comb", read);
                }
            }
            for write in block.writes.iter() {
                if dep.rw[*write].is_comb() {
                    writes.insert(*write);
                }
            }
        }
        let diff = part.comb_states.difference(&writes).cloned().collect_vec();
        if !diff.is_empty() {
            panic!(
                "comb state {:?} is not written by this thread",
                diff,
            );
        }
    }

    PartDesign {
        max_concat: design.max_concat,
        ext_fns: design.ext_fns.clone(),
        states: design.states.clone(),
        parts,
    }
}

pub fn print_pdesign_stats(pdesign: &PartDesign) {
    let mut table = Table::new("{:>}  {:>} {:>} {:>}  {:>} {:>} {:>}");
    table.add_row(Row::from_cells([
        "Id", "Blks", "Rsts", "Insts", "SReads", "SWrites", "CStates"
    ]));
    let mut total_blocks = 0;
    for (part_id, part) in pdesign.parts.iter().enumerate() {
        let total_insts = part.blocks.iter().map(|block| block.insts.len()).sum::<usize>();
        total_blocks += part.blocks.len();
        table.add_row(Row::from_cells([
            part_id.to_string(),
            part.blocks.len().to_string(),
            part.resets.len().to_string(),
            total_insts.to_string(),
            part.seq_reads.len().to_string(),
            part.seq_writes.len().to_string(),
            part.comb_states.len().to_string(),
        ]));
    }
    info!("Block Partition Stats:\n{}", table);
    info!("Total blocks: {}", total_blocks);
}
