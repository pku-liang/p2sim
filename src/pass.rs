use std::{collections::BTreeSet, ops::Range};

use index_vec_ext::{IndexExt, IndexVecExt, index_ext};
use itertools::Itertools;
use log::{info, warn};
use mt_kahypar::{Context, Hypergraph, Objective, Preset};
use petgraph::{
    Direction,
    algo::tred,
    graph::{DiGraph, NodeIndex, node_index},
    visit::IntoNeighbors,
};
use rustc_hash::{FxHashMap, FxHashSet};
use tabular::{Row, Table};

use crate::ir::*;

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

#[derive(Default, Clone)]
pub struct StateDep {
    pub reads: Vec<BlockId>,
    pub writes: Vec<BlockId>,
}

index_ext! { containers StateId, StateDep }

impl StateDep {
    pub fn is_comb(&self) -> bool {
        if self.reads.is_empty() || self.writes.is_empty() {
            false
        } else {
            self.writes.last().unwrap() < self.reads.first().unwrap()
        }
    }
    pub fn is_seq(&self) -> bool {
        if self.reads.is_empty() || self.writes.is_empty() {
            true
        } else {
            self.reads.last().unwrap() <= self.writes.first().unwrap()
        }
    }
}

pub struct DepInfo {
    pub states: Vec<StateDep>,
    pub cdep: DiGraph<(), ()>,
    pub sdep: DiGraph<(), StateId>,
}

impl DepInfo {
    pub fn is_sink(&self, blk_id: BlockId) -> bool {
        let has_seq_succ = self
            .sdep
            .neighbors_directed(blk_id.node_index(), Direction::Outgoing)
            .count()
            > 0;
        let has_no_comb_succ = self
            .cdep
            .neighbors_directed(blk_id.node_index(), Direction::Outgoing)
            .count()
            == 0;
        has_seq_succ || has_no_comb_succ
    }
    pub fn get_sink_states(&self, blk_id: BlockId) -> Vec<StateId> {
        self.sdep
            .edges_directed(blk_id.node_index(), Direction::Outgoing)
            .map(|x| *x.weight())
            .sorted()
            .unique()
            .collect_vec()
    }
}

pub trait NodeIdxExt {
    fn node_index(self) -> NodeIndex;
}
impl NodeIdxExt for BlockId {
    fn node_index(self) -> NodeIndex {
        node_index(self.0 as usize)
    }
}

pub fn get_dep_info(design: &Design) -> DepInfo {
    let mut states = vec![StateDep::default(); design.states.len()];
    for (blk_id, block) in design.blocks.enumerate_ids() {
        for read in &block.reads {
            states[*read].reads.push(blk_id);
        }
        for write in &block.writes {
            states[*write].writes.push(blk_id);
        }
    }
    let mut cdep = DiGraph::new();
    let mut sdep = DiGraph::new();
    for _ in &design.blocks {
        cdep.add_node(());
        sdep.add_node(());
    }
    for (sid, s) in states.enumerate_ids() {
        if s.is_comb() {
            for w in &s.writes {
                for r in &s.reads {
                    cdep.add_edge(w.node_index(), r.node_index(), ());
                }
            }
        } else if s.is_seq() {
            for r in &s.reads {
                for w in &s.writes {
                    if r != w {
                        cdep.add_edge(r.node_index(), w.node_index(), ());
                    }
                    sdep.add_edge(w.node_index(), r.node_index(), sid);
                }
            }
        } else {
            panic!("state {:?} is not comb or seq", design.states[sid].name);
        }
    }
    DepInfo { states, cdep, sdep }
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
    fn build<'ctx>(&'ctx self, ctx: &'ctx Context) -> Result<Hypergraph<'ctx>, mt_kahypar::Error> {
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
    pub seq_writes_local: BTreeSet<StateId>,
    pub seq_writes_shared: BTreeSet<StateId>,
    pub comb_states: BTreeSet<StateId>,
    pub blocks: Vec<BlockId>,
    pub sinks: Vec<BlockId>,
    pub resets: Vec<usize>,
}

pub struct PartDesign<'d> {
    pub design: &'d Design,
    pub parts: Vec<Part>,
    pub shared: BTreeSet<StateId>,
}

index_ext! {pub struct PartId(pub u32) for Part}

pub fn partition<'d>(design: &'d Design, k: usize) -> PartDesign<'d> {
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
    for (blk_id, _) in design.blocks.enumerate_ids() {
        if dep.is_sink(blk_id) {
            let sink_id = sinks.len();
            let size = dep.get_sink_states(blk_id).len() as i32 + 1;
            sinks.push(blk_id);
            sink_size.push(size);
            sink_map[blk_id.0 as usize] = Some(sink_id);
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
        if block.always {
            builder.add_edge(10000, get_edges(blk_id));
        }
    }
    for (sid, s) in dep.states.enumerate_ids() {
        if design.states[sid].element_count() > 32 {
            let weight = s.writes.len() as i32 / 32 + 1;
            let mut edges = vec![];
            for r in &s.reads {
                edges.extend(get_edges(*r));
            }
            for w in &s.writes {
                edges.extend(get_edges(*w));
            }
            builder.add_edge(weight, edges);
        }
    }

    info!("4. Partition");
    let mut part_id = vec![0; sinks.len()];
    if k != 1 {
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
        let part = hpg.partition().unwrap();
        part_id = part.extract_partition();
        assert!(part_id.len() == sinks.len());
    }

    info!("5. Extract partition");
    let mut parts = vec![Part::default(); k];
    let mut inside_map = vec![BTreeSet::default(); design.blocks.len()];
    for (i, sink) in sinks.iter().enumerate() {
        parts[part_id[i] as usize].sinks.push(*sink);
        inside_map[sink.0 as usize].insert(part_id[i] as usize);
    }
    for i in design.blocks.ids().rev() {
        let mut insides = BTreeSet::default();
        for succ in dep
            .cdep
            .neighbors_directed(i.node_index(), Direction::Outgoing)
        {
            insides.extend(inside_map[succ.index()].iter().cloned());
        }
        inside_map[i.0 as usize].extend(insides);
    }
    for i in design.blocks.ids() {
        for part_id in inside_map[i.0 as usize].iter() {
            parts[*part_id].blocks.push(i);
        }
    }
    for (reset_id, reset) in design.resets.iter().enumerate() {
        let mut need_add_reset = BTreeSet::default();
        for act in reset.acts.iter() {
            need_add_reset.extend(inside_map[act.0 as usize].iter().cloned());
        }
        for part_id in need_add_reset {
            parts[part_id].resets.push(reset_id);
        }
    }
    for part in parts.iter_mut() {
        let mut comb_writes = BTreeSet::default();
        for rst_id in &part.resets {
            let orig_reset = &design.resets[*rst_id];
            let reset = orig_reset.reset;
            if dep.states[reset].is_comb() {
                part.comb_states.insert(reset);
            } else if dep.states[reset].is_seq() {
                part.seq_reads.insert(reset);
            } else {
                panic!("reset {:?} is not comb or seq", reset);
            }
        }
        for blk_id in &part.blocks {
            for read in &design.blocks[*blk_id].reads {
                if dep.states[*read].is_comb() {
                    part.comb_states.insert(*read);
                } else if dep.states[*read].is_seq() {
                    part.seq_reads.insert(*read);
                } else {
                    panic!("read {:?} is not comb or seq", read);
                }
            }
            for write in &design.blocks[*blk_id].writes {
                if dep.states[*write].is_comb() {
                    comb_writes.insert(*write);
                }
            }
        }
        for sink in &part.sinks {
            for write in &design.blocks[*sink].writes {
                if dep.states[*write].is_seq() {
                    part.seq_writes.insert(*write);
                }
            }
        }
        let diff = part
            .comb_states
            .difference(&comb_writes)
            .cloned()
            .collect_vec();
        if !diff.is_empty() {
            let first = diff.first().unwrap();
            panic!(
                "comb state {:?} is not written by this thread",
                design.states[*first].name
            );
        }
    }

    let mut num_reads = FxHashMap::<StateId, usize>::default();
    let mut all_reads = BTreeSet::<StateId>::default();
    for part in parts.iter() {
        for read in part.seq_reads.iter() {
            *num_reads.entry(*read).or_insert(0) += 1;
            all_reads.insert(*read);
        }
    }
    let mut shared: BTreeSet<StateId> = num_reads
        .iter()
        .filter(|(_, cnt)| **cnt > 1)
        .map(|(sid, _)| sid)
        .cloned()
        .collect();
    for (sid, s) in design.states.enumerate_ids() {
        if s.is_ext {
            shared.insert(sid);
        }
    }
    let local_states: BTreeSet<StateId> = all_reads.difference(&shared).cloned().collect();
    for part in parts.iter_mut() {
        part.seq_writes_local = part
            .seq_writes
            .intersection(&local_states)
            .cloned()
            .collect();
        part.seq_writes_shared = part.seq_writes.difference(&local_states).cloned().collect();
    }
    PartDesign {
        design,
        parts,
        shared,
    }
}

pub fn print_part_stats(pd: &PartDesign) {
    let dep = get_dep_info(pd.design);
    let mut table =
        Table::new("{:>}  {:>} {:>} {:>} {:>}  {:>} {:>} {:>} {:>} {:>} {:>} {:>} {:>}");
    table.add_row(Row::from_cells([
        "Id",
        "Blks",
        "Sinks",
        "Rsts",
        "Insts",
        "SReads",
        "Local",
        "%",
        "SWrites",
        "Local",
        "%",
        "CStates",
        "Total Seq",
    ]));
    let mut total_blocks = 0;
    let mut read_cnt = FxHashMap::<StateId, usize>::default();
    for (_, part) in pd.parts.iter().enumerate() {
        for sr in part.seq_reads.iter() {
            *read_cnt.entry(*sr).or_insert(0) += 1;
        }
    }
    let local_states: BTreeSet<StateId> = read_cnt
        .into_iter()
        .filter(|(_, cnt)| *cnt == 1)
        .map(|(sid, _)| sid)
        .collect();
    for (part_id, part) in pd.parts.iter().enumerate() {
        let total_insts = part
            .blocks
            .iter()
            .map(|block| pd.design.blocks[*block].insts.len())
            .sum::<usize>();
        let mut total_seq = 0;
        for blk_id in part.blocks.iter() {
            for write in &pd.design.blocks[*blk_id].writes {
                if dep.states[*write].is_seq() {
                    total_seq += 1;
                }
            }
        }
        total_blocks += part.blocks.len();
        let local_reads = part.seq_reads.intersection(&local_states).count();
        let local_writes = part.seq_writes.intersection(&local_states).count();
        table.add_row(Row::from_cells([
            part_id.to_string(),
            part.blocks.len().to_string(),
            part.sinks.len().to_string(),
            part.resets.len().to_string(),
            total_insts.to_string(),
            part.seq_reads.len().to_string(),
            local_reads.to_string(),
            format!(
                "{:.2}%",
                local_reads as f64 / part.seq_reads.len() as f64 * 100.0
            ),
            part.seq_writes.len().to_string(),
            local_writes.to_string(),
            format!(
                "{:.2}%",
                local_writes as f64 / part.seq_writes.len() as f64 * 100.0
            ),
            part.comb_states.len().to_string(),
            total_seq.to_string(),
        ]));
    }
    info!("Block Partition Stats:\n{}", table);
    let orig_blocks = pd.design.blocks.len();
    let duplication = total_blocks as f64 / orig_blocks as f64 - 1.0;
    info!(
        "Blocks: {} => {} (Dup {:.2}%)",
        orig_blocks,
        total_blocks,
        duplication * 100.0
    );
}

pub struct TPart {
    pub states: Vec<State>,
    pub blocks: Vec<Block>,
    pub resets: Vec<Reset>,
}

impl TPart {
    pub fn get_act_map(&self) -> ActMap {
        let mut act_map: ActMap = FxHashMap::default();
        for (blk_id, block) in self.blocks.enumerate_ids() {
            for read in &block.reads {
                act_map.entry(*read).or_default().push(blk_id);
            }
        }
        act_map
    }
}

pub struct TDesign {
    pub name: String,
    pub max_concat: u32,
    pub ext_fns: Vec<String>,
    pub shared: Vec<State>,
    pub parts: Vec<TPart>,
}

fn update_block(block: &mut Block, state_map: &FxHashMap<StateId, StateId>) {
    for read in block.reads.iter_mut() {
        *read = state_map.get(read).cloned().unwrap();
    }
    for write in block.writes.iter_mut() {
        *write = state_map.get(write).cloned().unwrap();
    }
    for inst in block.insts.iter_mut() {
        match inst {
            Inst::Access { sid, .. } | Inst::WritePre { sid, .. } | Inst::WritePost { sid, .. } => {
                *sid = state_map.get(sid).cloned().unwrap();
            }
            Inst::CppCode { upd: Some(sid), .. } => {
                *sid = state_map.get(sid).cloned().unwrap();
            }
            Inst::CppCode { upd: None, .. } => {}
        }
    }
}

pub fn build_tdesign(pd: &PartDesign) -> TDesign {
    let mut shared: Vec<State> = vec![];
    let mut shared_map = FxHashMap::<StateId, StateId>::default();
    let mut parts = vec![];
    for sid in pd.shared.iter() {
        let new_sid = shared.add(pd.design.states[*sid].clone());
        shared_map.insert(*sid, new_sid);
    }
    for (_, part) in pd.parts.enumerate_ids() {
        let mut states: Vec<State> = vec![];
        let mut state_map = FxHashMap::<StateId, StateId>::default();
        let mut all_states = BTreeSet::<StateId>::default();
        let mut blocks: Vec<Block> = vec![];
        for blk_id in part.blocks.iter() {
            all_states.extend(pd.design.blocks[*blk_id].reads.iter().cloned());
            all_states.extend(pd.design.blocks[*blk_id].writes.iter().cloned());
        }
        for rst_id in part.resets.iter() {
            all_states.insert(pd.design.resets[*rst_id].reset);
        }
        for &sid in all_states.iter() {
            let mut state = pd.design.states[sid].clone();
            if let Some(mirror) = shared_map.get(&sid) {
                state.mirror = Some(*mirror);
            } else if part.seq_writes.contains(&sid) || part.seq_reads.contains(&sid) || part.comb_states.contains(&sid) {
                // do nothing
            } else {
                // no need to be written or read
                state.is_disabled = true;
            }
            state_map.insert(sid, states.add(state));
        }
        for blk_id in part.blocks.iter() {
            let orig_block = &pd.design.blocks[*blk_id];
            let mut new_block = orig_block.clone();
            update_block(&mut new_block, &state_map);
            blocks.push(new_block);
        }
        let mut block_id_map = FxHashMap::<BlockId, BlockId>::default();
        for (i, blk_id) in part.blocks.iter().enumerate() {
            block_id_map.insert(*blk_id, BlockId(i as u32));
        }
        let mut resets = vec![];
        for rst_id in part.resets.iter() {
            let orig_reset = &pd.design.resets[*rst_id];
            let mut new_insts = vec![];
            for inst in orig_reset.insts.iter() {
                match inst {
                    Inst::CppCode {
                        inst,
                        upd: Some(sid),
                    } => {
                        if let Some(new_sid) = state_map.get(sid) {
                            new_insts.push(Inst::CppCode {
                                inst: inst.clone(),
                                upd: Some(*new_sid),
                            })
                        }
                    }
                    Inst::CppCode { inst, upd: None } => new_insts.push(Inst::CppCode {
                        inst: inst.clone(),
                        upd: None,
                    }),
                    _ => {}
                }
            }
            let mut new_acts = vec![];
            for act in orig_reset.acts.iter() {
                if let Some(new_act) = block_id_map.get(act) {
                    new_acts.push(*new_act);
                }
            }
            let new_reset = Reset {
                reset: *state_map.get(&orig_reset.reset).unwrap(),
                insts: new_insts,
                acts: new_acts,
            };
            resets.push(new_reset);
        }
        let new_part = TPart {
            states,
            blocks,
            resets,
        };
        parts.push(new_part);
    }
    TDesign {
        name: pd.design.name.clone(),
        max_concat: pd.design.max_concat,
        ext_fns: pd.design.ext_fns.clone(),
        shared,
        parts,
    }
}
