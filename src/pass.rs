use std::{
    collections::{BTreeMap, BTreeSet, VecDeque},
    ops::Range,
};

use index_vec_ext::{IndexExt, IndexVecExt, index_ext};
use itertools::Itertools;
use log::{info, warn};
use mt_kahypar::{Context, Hypergraph, Objective, Preset};
use petgraph::{
    Direction,
    graph::{DiGraph, NodeIndex, node_index},
};
use rustc_hash::{FxHashMap, FxHashSet};
use tabular::{Row, Table};

use crate::{act::OrigBlockRecord, ir::*};

pub type ActMap = FxHashMap<StateId, Vec<BlockId>>;

pub fn get_act_map(part: &TPart) -> ActMap {
    let mut act_map: ActMap = FxHashMap::default();
    for (blk_id, block) in part.blocks.enumerate_ids() {
        for read in &block.reads {
            act_map.entry(*read).or_default().push(blk_id);
        }
    }
    act_map
}

pub fn verify_act_map(part: &TPart) {
    let act_map = get_act_map(part);
    for block in &part.blocks {
        for inst in &block.insts {
            if let Inst::WritePost { sid, acts, .. } = inst {
                let act_map_acts = act_map.get(sid).map(|x| &x[..]).unwrap_or(&[]);
                for a in acts {
                    assert!(
                        act_map_acts.contains(a),
                        "Act {} doesn't include {:?}, ({:?})",
                        part.states[*sid].name,
                        a,
                        act_map_acts
                    );
                }
                if acts.len() < act_map_acts.len() {
                    warn!(
                        "Activates more blocks than gsim: {} {} => {}",
                        part.states[*sid].name,
                        acts.len(),
                        act_map_acts.len()
                    );
                }
            }
        }
    }
}

pub fn verify_never_act(part: &TPart) {
    let mut all_acts = FxHashSet::default();
    for block in &part.blocks {
        for inst in &block.insts {
            if let Inst::WritePost { acts, .. } = inst {
                for a in acts {
                    all_acts.insert(*a);
                }
            }
        }
    }
    for reset in &part.resets {
        for act in &reset.acts {
            all_acts.insert(*act);
        }
    }
    let mut never_act = vec![];
    for (blk_id, block) in part.blocks.enumerate_ids() {
        if !block.always && !all_acts.contains(&blk_id) {
            never_act.push(blk_id.0);
        }
    }
    warn!("Never act blocks: {:?}", never_act);
}

#[derive(Default, Clone, Debug)]
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

pub struct DepInfo<'d> {
    pub part: &'d TPart,
    pub states: Vec<StateDep>,
    pub cdep: DiGraph<usize, ()>,
    pub sdep: DiGraph<(), StateId>,
}

impl<'d> DepInfo<'d> {
    pub fn is_sink(&self, blk_id: BlockId) -> bool {
        let has_seq_write = self.part.blocks[blk_id]
            .writes
            .iter()
            .any(|write| self.states[*write].is_seq());
        let has_no_comb_succ = self
            .cdep
            .neighbors_directed(blk_id.node_index(), Direction::Outgoing)
            .count()
            == 0;
        has_seq_write || has_no_comb_succ
    }
    pub fn get_sink_states(&self, blk_id: BlockId) -> Vec<StateId> {
        self.part.blocks[blk_id]
            .writes
            .iter()
            .filter_map(|write| {
                if self.states[*write].is_seq() {
                    Some(*write)
                } else {
                    None
                }
            })
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

pub fn get_dep_info<'d>(part: &'d TPart) -> DepInfo<'d> {
    let mut states = vec![StateDep::default(); part.states.len()];
    for (blk_id, block) in part.blocks.enumerate_ids() {
        for read in &block.reads {
            states[*read].reads.push(blk_id);
        }
        for write in &block.writes {
            states[*write].writes.push(blk_id);
        }
    }
    let mut cdep = DiGraph::new();
    let mut sdep = DiGraph::new();
    for b in &part.blocks {
        cdep.add_node(b.insts.len());
        sdep.add_node(());
    }
    for (sid, s) in states.enumerate_ids() {
        if part.states[sid].is_ext {
            if !s.is_seq() {
                eprintln!("ext state {:?} is not seq {:?}", part.states[sid], s);
            }
            assert!(
                s.is_seq(),
                "ext state {:?} is not seq",
                part.states[sid].name
            );
        }
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
            panic!("state {:?} is not comb or seq", part.states[sid].name);
        }
    }
    DepInfo {
        part,
        states,
        cdep,
        sdep,
    }
}

pub fn leaglize_empty_block(part: &mut TPart) {
    for (blk_id, block) in part.blocks.enumerate_ids_mut() {
        if block
            .insts
            .iter()
            .all(|inst| matches!(inst, Inst::Access { .. }))
        {
            block.insts.clear();
            block.reads.clear();
            block.writes.clear();
            warn!("Clear empty block: {:?}", blk_id);
        }
    }
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

#[derive(Clone)]
pub struct Part<'d> {
    pub orig: &'d TPart,
    pub blocks: Vec<BlockId>,
    pub sinks: Vec<BlockId>,
    pub resets: Vec<BlockId>,
}

pub struct PartDesign<'d> {
    pub design: &'d Design,
    pub parts: Vec<Part<'d>>,
    pub dep: PartDep,
}

#[derive(Default, Clone, Debug)]
pub struct StateInfo {
    pub is_shared: bool,
    pub read: BTreeSet<PartId>,
    pub write: BTreeSet<PartId>,
}

impl StateInfo {
    pub fn get_access_type(&self, part_id: PartId) -> AccessType {
        if self.read.contains(&part_id) && self.write.contains(&part_id) {
            AccessType::ReadWrite
        } else if self.read.contains(&part_id) {
            AccessType::ReadOnly
        } else if self.write.contains(&part_id) {
            AccessType::WriteOnly
        } else {
            AccessType::BlackHole
        }
    }
}

index_ext! { containers StateId, StateInfo }
index_ext! {pub struct PartId(pub u32) for Part<'d>}

pub struct PartDep {
    pub states: Vec<StateInfo>,
}

impl<'d> Part<'d> {
    pub fn new(design: &'d TPart) -> Self {
        Self {
            orig: design,
            blocks: vec![],
            sinks: vec![],
            resets: vec![],
        }
    }
}

pub struct Config {
    pub disable_node_weight: bool,
    pub disable_comm_cost: bool,
}

pub fn partition<'d>(
    design: &'d Design,
    k: usize,
    cost: impl Fn(BlockId) -> i32,
    cfg: &Config,
) -> PartDesign<'d> {
    let part = &design.part;
    info!("Start repcut partition");
    info!("1. Build transitive closure");
    let dep = get_dep_info(part);
    // let toposort = (0..design.blocks.len()).map(node_index).collect_vec();
    // let (succs, _) = tred::dag_to_toposorted_adjacency_list::<_, u32>(&dep.cdep, &toposort);
    // let (_, tclos) = tred::dag_transitive_reduction_closure::<_, u32>(&succs);

    info!("2. Find sink nodes");
    let mut sinks = vec![];
    let mut sink_size = vec![];
    let mut sink_map = vec![None; part.blocks.len()];
    let mut blk_in_sinks = vec![BTreeSet::default(); part.blocks.len()];
    for (blk_id, _) in part.blocks.enumerate_ids() {
        if dep.is_sink(blk_id) {
            let sink_id = sinks.len();
            let size = dep.get_sink_states(blk_id).len() as i32 + 1;
            sinks.push(blk_id);
            sink_size.push(size);
            sink_map[blk_id.0 as usize] = Some(sink_id);
            blk_in_sinks[blk_id.0 as usize].insert(sink_id);
        }
    }

    for blk_id in part.blocks.ids().rev() {
        let succ_iter = dep
            .cdep
            .neighbors_directed(blk_id.node_index(), Direction::Outgoing);
        let mut succs = BTreeSet::default();
        for succ in succ_iter {
            succs.extend(blk_in_sinks[succ.index()].iter().cloned());
        }
        blk_in_sinks[blk_id.0 as usize].extend(succs);
        let bis = &blk_in_sinks[blk_id.0 as usize];
        if !cfg.disable_node_weight {
            let cost = (cost(blk_id) + bis.len() as i32) / bis.len() as i32;
            for sink in bis {
                sink_size[*sink] += cost;
            }
        } else {
            let cost = (100 + bis.len() as i32) / bis.len() as i32;
            for sink in bis {
                sink_size[*sink] += cost;
            }
        }
    }

    info!("3. Build Hypergraph");
    let mut builder = HPG::new(sink_size);
    let get_edges = |blk_id: BlockId| blk_in_sinks[blk_id.0 as usize].iter().cloned();
    for (blk_id, block) in part.blocks.enumerate_ids() {
        let weight = block.insts.len() as i32 / 5 + 1;
        builder.add_edge(weight, get_edges(blk_id));
        if block.always {
            builder.add_edge(10000, get_edges(blk_id));
        }
    }
    for (sid, s) in dep.states.enumerate_ids() {
        if part.states[sid].element_count() > 32 {
            let weight = 2 * part.states[sid].element_count() as i32;
            let mut edges = vec![];
            for r in &s.reads {
                edges.extend(get_edges(*r));
            }
            for w in &s.writes {
                edges.extend(get_edges(*w));
            }
            edges.sort();
            edges.dedup();
            builder.add_edge(weight, edges);
        }
        if s.is_seq() && s.writes.len() > 1 {
            let mut edges = vec![];
            for r in &s.reads {
                edges.extend(get_edges(*r));
            }
            for w in &s.writes {
                edges.extend(get_edges(*w));
            }
            edges.sort();
            edges.dedup();
            builder.add_edge(10000, edges);
        }
    }
    info!("4. Partition");
    let mut part_id = vec![0; sinks.len()];
    if k != 1 {
        let ctx = Context::builder()
            .preset(Preset::Deterministic)
            .k(k as i32)
            .epsilon(0.1)
            .objective(Objective::Cut)
            .seed(42)
            .verbose(true)
            .build()
            .unwrap();
        let hpg = builder.build(&ctx).unwrap();
        let part = hpg.partition().unwrap();
        part_id = part.extract_partition();
        assert!(part_id.len() == sinks.len());
    }

    info!("5. Extract partition");
    let mut parts = vec![Part::new(part); k];
    let mut blk_in_parts = vec![BTreeSet::default(); part.blocks.len()];

    for (i, sink) in sinks.iter().enumerate() {
        blk_in_parts[sink.0 as usize].insert(part_id[i] as usize);
        parts[part_id[i] as usize].sinks.push(*sink);
    }
    for blk_id in part.blocks.ids().rev() {
        let mut insides = BTreeSet::default();
        let succ_iter = dep
            .cdep
            .neighbors_directed(blk_id.node_index(), Direction::Outgoing);
        for succ in succ_iter {
            insides.extend(blk_in_parts[succ.index()].iter().cloned());
        }
        blk_in_parts[blk_id.0 as usize].extend(insides);
    }
    for blk_id in part.blocks.ids() {
        for part_id in blk_in_parts[blk_id.0 as usize].iter() {
            parts[*part_id].blocks.push(blk_id);
        }
    }
    for (rst_id, reset) in part.resets.enumerate_ids() {
        let mut need_add_reset = FxHashSet::default();
        for act in reset.acts.iter() {
            for part_id in blk_in_parts[act.0 as usize].iter() {
                need_add_reset.insert(*part_id);
            }
        }
        for part_id in need_add_reset {
            parts[part_id].resets.push(rst_id);
        }
    }

    let mut states = vec![StateInfo::default(); part.states.len()];
    for (part_id, new_part) in parts.enumerate_ids() {
        for sink in new_part.sinks.iter() {
            for write in &part.blocks[*sink].writes {
                states[*write].write.insert(part_id);
                states[*write].is_shared = true;
            }
        }
        for rst_id in new_part.resets.iter() {
            let reset = part.resets[*rst_id].reset;
            if dep.states[reset].is_seq() {
                states[reset].read.insert(part_id);
            }
        }
        for blk_id in new_part.blocks.iter() {
            for read in &part.blocks[*blk_id].reads {
                if dep.states[*read].is_seq() {
                    states[*read].read.insert(part_id);
                }
            }
        }
    }
    for (sid, s) in states.enumerate_ids_mut() {
        if part.states[sid].is_ext {
            s.is_shared = true;
        } else if s.read.is_empty() || s.write.is_empty() {
            s.is_shared = false;
        } else if s.read.len() == 1 && s.read == s.write {
            s.is_shared = false;
        }
    }

    PartDesign {
        design,
        parts,
        dep: PartDep { states },
    }
}

pub fn print_part_stats(pd: &PartDesign) {
    let mut table = Table::new("{:>}  {:>} {:>} {:>} {:>}");
    table.add_row(Row::from_cells(["Id", "Blks", "Sinks", "Rsts", "Insts"]));
    let mut total_blocks = 0;
    for (part_id, part) in pd.parts.iter().enumerate() {
        let total_insts = part
            .blocks
            .iter()
            .map(|block| pd.design.part.blocks[*block].insts.len())
            .sum::<usize>();
        total_blocks += part.blocks.len();
        table.add_row(Row::from_cells([
            part_id.to_string(),
            part.blocks.len().to_string(),
            part.sinks.len().to_string(),
            part.resets.len().to_string(),
            total_insts.to_string(),
        ]));
    }
    info!(
        "Block Partition Stats:\n{}",
        table.to_string().strip_suffix("\n").unwrap()
    );
    let orig_blocks = pd.design.part.blocks.len();
    let duplication = total_blocks as f64 / orig_blocks as f64 - 1.0;
    info!(
        "Blocks: {} => {} (Dup {:.2}%)",
        orig_blocks,
        total_blocks,
        duplication * 100.0
    );
}

// pub struct TPart {
//     pub states: Vec<State>,
//     pub blocks: Vec<Block>,
//     pub resets: Vec<Reset>,
// }

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
    pub ext_states: Vec<ExternalState>,
    pub shared_acts: u32,
    pub shared: Vec<State>,
    pub parts: Vec<TPart>,
}

index_ext! { containers PartId, TPart }

struct PartRewrite {
    pub id_map: FxHashMap<StateId, StateId>,
    pub blk_map: FxHashMap<BlockId, BlockId>,
}

impl PartRewrite {
    pub fn update_block(&self, block: &mut Block) {
        for read in block.reads.iter_mut() {
            *read = *self.id_map.get(read).unwrap();
        }
        for write in block.writes.iter_mut() {
            *write = *self.id_map.get(write).unwrap();
        }
        for inst in block.insts.iter_mut() {
            match inst {
                Inst::Access { sid, .. }
                | Inst::WritePre { sid, .. }
                | Inst::WritePost { sid, .. }
                | Inst::CppCode { upd: Some(sid), .. } => {
                    *sid = *self.id_map.get(sid).unwrap();
                }
                Inst::CppCode { upd: None, .. } => {}
            }
        }
    }
    pub fn update_reset(&self, reset: &mut Reset) {
        let mut new_acts = vec![];
        for act in &reset.acts {
            if let Some(new_act) = self.blk_map.get(act) {
                new_acts.push(*new_act);
            }
        }
        reset.acts = new_acts;
        reset.reset = *self.id_map.get(&reset.reset).unwrap();
        let mut new_insts = vec![];
        for inst in reset.insts.drain(..) {
            match inst {
                Inst::CppCode {
                    upd: Some(sid),
                    inst,
                } => {
                    if let Some(new_sid) = self.id_map.get(&sid) {
                        new_insts.push(Inst::CppCode {
                            upd: Some(*new_sid),
                            inst: inst.clone(),
                        });
                    }
                }
                inst @ Inst::CppCode { upd: None, .. } => {
                    new_insts.push(inst);
                }
                _ => {}
            }
        }
        reset.insts = new_insts;
    }
}

pub fn build_tdesign(pd: &PartDesign) -> TDesign {
    let mut shared_ids = vec![];
    for (sid, s) in pd.dep.states.enumerate_ids() {
        if s.is_shared {
            shared_ids.push(sid);
        }
    }
    shared_ids.sort_by_cached_key(|id| {
        let first_write = pd.dep.states[*id]
            .write
            .first()
            .cloned()
            .unwrap_or(PartId(0));
        // let first_read = pd.dep.states[*id].read.first().cloned().unwrap_or(PartId(0));
        // (first_write, first_read)
        first_write
    });
    let mut shared: Vec<State> = vec![];
    let mut shared_id_map = FxHashMap::default();
    for id in shared_ids.iter() {
        let new_id = shared.add(pd.design.part.states[*id].clone());
        shared_id_map.insert(*id, new_id);
    }
    let mut parts = vec![];
    for (part_id, part) in pd.parts.enumerate_ids() {
        let mut all_states = BTreeSet::<StateId>::default();
        for rst_id in part.resets.iter() {
            all_states.insert(pd.design.part.resets[*rst_id].reset);
        }
        for blk_id in part.blocks.iter() {
            all_states.extend(pd.design.part.blocks[*blk_id].reads.iter().cloned());
            all_states.extend(pd.design.part.blocks[*blk_id].writes.iter().cloned());
        }
        let mut all_states = all_states.into_iter().collect_vec();
        all_states.sort_by_cached_key(|id| {
            let access_ty = pd.dep.states[*id].get_access_type(part_id);
            if access_ty.has_write() {
                (false, shared_id_map.get(id).cloned().unwrap_or(StateId(0)))
            } else {
                (true, shared_id_map.get(id).cloned().unwrap_or(StateId(0)))
            }
        });
        let mut states: Vec<State> = vec![];
        let mut id_map = FxHashMap::<StateId, StateId>::default();
        for sid in all_states.iter() {
            let mut state = part.orig.states[*sid].clone();
            let info = &pd.dep.states[*sid];
            if info.is_shared {
                let access_ty = info.get_access_type(part_id);
                if access_ty != AccessType::BlackHole {
                    state.mirror = Some(Mirror {
                        shared_id: shared_id_map.get(sid).cloned().unwrap(),
                        ty: access_ty,
                    })
                }
            }
            id_map.insert(*sid, states.add(state));
        }
        let mut blk_map = FxHashMap::<BlockId, BlockId>::default();
        for (i, blk_id) in part.blocks.iter().enumerate() {
            blk_map.insert(*blk_id, BlockId(i as u32));
        }
        let rewrite = PartRewrite { id_map, blk_map };
        let mut blocks = vec![];
        let mut resets = vec![];
        for blk_id in part.blocks.iter() {
            let mut new_block = pd.design.part.blocks[*blk_id].clone();
            new_block.orig_ids = vec![*blk_id];
            rewrite.update_block(&mut new_block);
            blocks.push(new_block);
        }
        for rst_id in part.resets.iter() {
            let mut new_reset = pd.design.part.resets[*rst_id].clone();
            rewrite.update_reset(&mut new_reset);
            resets.push(new_reset);
        }
        parts.push(TPart {
            states,
            blocks,
            resets,
            pulls: vec![],
        });
    }
    // let shared = pd
    //     .dep
    //     .shared
    //     .iter()
    //     .map(|sid| pd.design.states[*sid].clone())
    //     .collect_vec();
    TDesign {
        name: pd.design.name.clone(),
        max_concat: pd.design.max_concat,
        ext_fns: pd.design.ext_fns.clone(),
        ext_states: vec![],
        shared,
        shared_acts: 0,
        parts,
    }
}

fn has_non_speculative_side_effect(block: &Block) -> bool {
    block.insts.iter().any(|inst| match inst {
        Inst::CppCode { inst, .. } => {
            inst.contains("gAssert(")
                || inst.contains("gprintf(")
                || inst.contains("printf(")
                || inst.contains("fprintf(")
                || inst.contains("assert(")
                || inst.contains("assert (")
                || inst.contains("abort(")
                || inst.contains("exit(")
        }
        _ => false,
    })
}

pub fn classify_speculative_part(part: &mut TPart, part_id: PartId, wait_remote: bool) {
    let classes = {
        let dep = get_dep_info(part);
        let mut non_speculative = vec![false; part.blocks.len()];
        let mut observer = vec![false; part.blocks.len()];
        let mut queue = VecDeque::new();

        for (block_id, block) in part.blocks.enumerate_ids() {
            let touches_array = block
                .reads
                .iter()
                .chain(block.writes.iter())
                .any(|state_id| part.states[*state_id].is_array());
            let reads_remote = block.reads.iter().any(|state_id| {
                part.states[*state_id]
                    .mirror
                    .as_ref()
                    .is_some_and(|mirror| mirror.ty.has_read())
            });
            observer[block_id.index()] = has_non_speculative_side_effect(block);
            if touches_array
                || (wait_remote && reads_remote)
                || block.always
                || observer[block_id.index()]
            {
                non_speculative[block_id.index()] = true;
                queue.push_back(block_id);
            }
        }

        // The early pass must be closed over same-cycle dependencies. If a
        // block waits for the full state, all of its combinational successors
        // must also wait. Cross-cycle sdep edges intentionally do not propagate.
        while let Some(block_id) = queue.pop_front() {
            for successor in dep
                .cdep
                .neighbors_directed(block_id.node_index(), Direction::Outgoing)
            {
                let successor = BlockId(successor.index() as u32);
                if !non_speculative[successor.index()] {
                    non_speculative[successor.index()] = true;
                    queue.push_back(successor);
                }
            }
        }

        non_speculative
            .into_iter()
            .zip(observer)
            .map(|(non_speculative, observer)| (!non_speculative, observer))
            .collect_vec()
    };

    let mut speculative_blocks = 0usize;
    let mut speculative_insts = 0usize;
    let total_insts = part
        .blocks
        .iter()
        .map(|block| block.insts.len())
        .sum::<usize>();
    for (block_id, block) in part.blocks.enumerate_ids_mut() {
        let (speculative, observer) = classes[block_id.index()];
        block.speculative = speculative;
        block.observer = observer;
        if block.speculative {
            speculative_blocks += 1;
            speculative_insts += block.insts.len();
        }
    }
    info!(
        "Speculative partition {}: blocks={}/{} ({:.1}%), insts={}/{} ({:.1}%)",
        part_id.index(),
        speculative_blocks,
        part.blocks.len(),
        speculative_blocks as f64 * 100.0 / part.blocks.len().max(1) as f64,
        speculative_insts,
        total_insts,
        speculative_insts as f64 * 100.0 / total_insts.max(1) as f64,
    );
}

pub fn classify_speculative(design: &mut TDesign, wait_remote: bool) {
    for (part_id, part) in design.parts.enumerate_ids_mut() {
        classify_speculative_part(part, part_id, wait_remote);
    }
}

pub fn solve_linear_part(dfg: &DiGraph<usize, ()>, bound: usize) -> Vec<Range<usize>> {
    let mut cost = vec![(usize::MAX, 0); dfg.node_count() + 1];
    for i in 0..dfg.node_count() {
        let mut accu_size = 0;
        let mut cur_cost = cost[i].0;
        for j in i..dfg.node_count() {
            let node = node_index(j);
            accu_size += dfg[node];
            if j > i && accu_size > bound {
                break;
            }
            let succ_edges = dfg.neighbors_directed(node, Direction::Outgoing);
            let prev_edges = dfg
                .neighbors_directed(node, Direction::Incoming)
                .filter(|prev| prev.index() >= i);
            cur_cost += succ_edges.count() - prev_edges.count();
            let new_cost = (cur_cost, i);
            if new_cost < cost[j + 1] {
                cost[j + 1] = new_cost;
            }
        }
    }
    let mut res = vec![];
    let mut cur = dfg.node_count();
    while cur > 0 {
        res.push(cost[cur].1..cur);
        cur = cost[cur].1;
    }
    res.reverse();
    res
}

pub fn merge_block(blocks: &[Block]) -> Block {
    let speculative = blocks
        .first()
        .map(|block| block.speculative)
        .unwrap_or(false);
    let observer = blocks.first().map(|block| block.observer).unwrap_or(false);
    assert!(
        blocks
            .iter()
            .all(|block| block.speculative == speculative && block.observer == observer),
        "cannot merge blocks from different speculative classes"
    );
    let always = blocks.iter().any(|b| b.always);
    let next = vec![];
    let mut writes = BTreeSet::new();
    let mut reads = BTreeSet::new();
    for blk in blocks {
        for read in &blk.reads {
            if !writes.contains(read) {
                reads.insert(*read);
            }
        }
        writes.extend(blk.writes.iter().cloned());
    }
    let insts = blocks
        .iter()
        .flat_map(|b| b.insts.iter().cloned())
        .collect_vec();
    Block {
        orig_ids: blocks
            .iter()
            .flat_map(|b| b.orig_ids.iter().cloned())
            .collect_vec(),
        always,
        next,
        writes: writes.into_iter().collect_vec(),
        reads: reads.into_iter().collect_vec(),
        insts,
        speculative,
        observer,
        ..Block::default()
    }
}

pub fn linear_coarsen_part(part: &mut TPart) {
    let dep = get_dep_info(part);
    let sol = solve_linear_part(&dep.cdep, 140);
    let mut class_preserving_sol = vec![];
    for range in sol {
        let mut start = range.start;
        while start < range.end {
            let class = (part.blocks[start].speculative, part.blocks[start].observer);
            let mut end = start + 1;
            while end < range.end
                && (part.blocks[end].speculative, part.blocks[end].observer) == class
            {
                end += 1;
            }
            class_preserving_sol.push(start..end);
            start = end;
        }
    }
    let mut new_blocks: Vec<Block> = vec![];
    let mut state_reads = vec![0; part.states.len()];
    let mut state_writes = vec![None; part.states.len()];
    let mut block_id_map = vec![];
    for (i, s) in class_preserving_sol.iter().enumerate() {
        let new_block = merge_block(&part.blocks[s.clone()]);
        for read in new_block.reads.iter() {
            state_reads[*read] += 1;
        }
        for write in new_block.writes.iter() {
            state_writes[write.index()] = Some(i);
        }
        let id = new_blocks.add(new_block);
        block_id_map.extend(s.clone().map(|_| id));
    }
    // for (sid, s) in dep.states.enumerate_ids() {
    //     if s.is_comb() && state_reads[sid] == 0 {
    //         assert!(part.states[sid].mirror.is_none());
    //         let write = state_writes[sid.index()].unwrap();
    //         let state_decl = gen_state_decl(&part.states[sid]);
    //         let inst = Inst::CppCode {
    //             inst: state_decl,
    //             upd: None,
    //         };
    //         new_blocks[write].insts.insert(0, inst);
    //         part.states[sid].is_disabled = true;
    //     }
    // }
    // for blk in new_blocks.iter_mut() {
    //     blk.insts.retain(|inst| match inst {
    //         Inst::Access { sid, .. } | Inst::WritePre { sid, .. } | Inst::WritePost { sid, .. } => {
    //             !part.states[*sid].is_disabled
    //         }
    //         _ => true,
    //     })
    // }
    for rst in part.resets.iter_mut() {
        let mut new_acts = vec![];
        for act in rst.acts.iter() {
            new_acts.push(block_id_map[act.0 as usize]);
        }
        rst.acts = new_acts.into_iter().sorted().unique().collect_vec();
    }
    info!(
        "  Linear coarsen: {} => {}",
        part.blocks.len(),
        new_blocks.len()
    );
    part.blocks = new_blocks;
}

pub fn linear_coarsen(design: &mut TDesign) {
    info!("Linear coarsen: {} parts", design.parts.len());
    for part in design.parts.iter_mut() {
        linear_coarsen_part(part);
    }
}

#[derive(Default, Clone, Debug)]
pub struct SStateDep {
    pub read: Vec<(PartId, StateId)>,
    pub write: Vec<(PartId, StateId)>,
}

#[derive(Default, Clone, Debug, Copy)]
pub struct ThState {
    pub shared_id: StateId,
    pub local_id: StateId,
}

#[derive(Default, Clone, Debug)]
pub struct ThreadDep {
    pub read: Vec<ThState>,
    pub write: Vec<ThState>,
}

index_ext! {containers StateId, SStateDep}

pub struct SharedDep {
    pub states: Vec<SStateDep>,
    pub threads: Vec<ThreadDep>,
}

index_ext! {containers PartId, ThreadDep}

pub fn get_shared_dep(td: &TDesign) -> SharedDep {
    let mut states = vec![SStateDep::default(); td.shared.len()];
    let mut threads = vec![ThreadDep::default(); td.parts.len()];
    for (part_id, part) in td.parts.enumerate_ids() {
        for (sid, s) in part.states.enumerate_ids() {
            if let Some(mirror) = &s.mirror {
                if mirror.ty == AccessType::ReadOnly {
                    states[mirror.shared_id].read.push((part_id, sid));
                    threads[part_id].read.push(ThState {
                        shared_id: mirror.shared_id,
                        local_id: sid,
                    });
                }
                if mirror.ty.has_write() {
                    states[mirror.shared_id].write.push((part_id, sid));
                    threads[part_id].write.push(ThState {
                        shared_id: mirror.shared_id,
                        local_id: sid,
                    });
                }
            }
        }
    }
    SharedDep { states, threads }
}

#[derive(Default)]
pub struct LayoutHelper {
    pub offset: usize,
    pub names: FxHashSet<String>,
    pub states: Vec<State>,
}

impl LayoutHelper {
    pub fn next(&self, state: &State) -> Range<usize> {
        let layout = state.get_layout();
        let start = self.offset.next_multiple_of(layout.align);
        let end = start + layout.size;
        return start..end;
    }
    pub fn add_pad(&mut self, size: usize) {
        self.states.push(State {
            name: format!("_pad_{}", self.states.len()),
            width: 8,
            dims: vec![size],
            ..State::default()
        });
    }
    pub fn get_name(&mut self, name: &str) -> String {
        let mut name = name.to_string();
        if self.names.contains(&name) {
            let mut idx = 0;
            while self.names.contains(&format!("{}_{}", name, idx)) {
                idx += 1;
            }
            name = format!("{}_{}", name, idx);
        }
        self.names.insert(name.clone());
        name
    }
    pub fn add(&mut self, state: &State) -> StateId {
        let range = self.next(state);
        if range.start != self.offset {
            self.add_pad(range.start - self.offset);
        }
        let name = self.get_name(&state.name);
        let sid = self.states.add(State {
            name,
            ..state.clone()
        });
        self.offset = range.end;
        sid
    }
    pub fn add_align(&mut self, align: usize) {
        let pos = self.offset.next_multiple_of(align);
        if pos != self.offset {
            self.add_pad(pos - self.offset);
        }
    }
    pub fn add_comment(&mut self, comment: &str) {
        self.states.push(State {
            name: format!("_comment_{}", comment),
            width: 8,
            dims: vec![0],
            is_ext: false,
            is_local: false,
            mirror: None,
            is_disabled: false,
        })
    }
}

#[derive(Debug)]
pub struct ExternalState {
    pub name: String,
    pub width: usize,
    pub acts: Vec<(StateId, usize)>,
}

pub fn compute_shared_layout(td: &mut TDesign) {
    let mut helper = LayoutHelper::default();
    let dep = get_shared_dep(td);
    let mut act_id = 0;
    let mut act_map = BTreeMap::<StateId, ExternalState>::default();
    for (th, th_dep) in dep.threads.enumerate_ids() {
        let mut wr_rd = BTreeMap::<PartId, Vec<_>>::new();
        for r in th_dep.read.iter() {
            let num_writes = dep.states[r.shared_id].write.len();
            if num_writes > 1 {
                let s = &td.shared[r.shared_id];
                println!("{:?} {:?}", s, dep.states[r.shared_id].write);
            }
            assert!(num_writes <= 1);
            let write = dep.states[r.shared_id]
                .write
                .first()
                .unwrap_or(&(PartId(u32::MAX), StateId(0)))
                .clone();
            wr_rd.entry(write.0).or_default().push((*r, write.1));
        }
        for (wth, rds) in wr_rd.iter_mut() {
            rds.sort_by_key(|x| x.1);
            if *wth == th {
                continue;
            }
            let mut comm_blk_id = 0;
            helper.add_comment(&format!("W{}_R{}_{}", wth.index(), th.index(), comm_blk_id));
            let mut last_offset = helper.offset;
            let mut pull_tasks = vec![];
            let mut push_tasks = vec![];
            let mut pulls = vec![];
            let mut pushes = vec![];
            let mut ext_acts: Vec<(StateId, StateId)> = vec![];
            for (r, _) in rds.iter() {
                let s = &td.shared[r.shared_id];
                let next = helper.next(s);
                // cross cache line
                if next.end >= last_offset + 64 {
                    helper.add_align(64);
                    comm_blk_id += 1;
                    helper.add_comment(&format!(
                        "W{}_R{}_{}",
                        wth.index(),
                        th.index(),
                        comm_blk_id
                    ));
                    last_offset = helper.offset;
                    pull_tasks.push(PullTask {
                        act_id,
                        pulls: std::mem::take(&mut pulls),
                    });
                    push_tasks.push(PushTask {
                        act_id,
                        pushes: std::mem::take(&mut pushes),
                    });
                    for (orig_shared_id, new_shared_id) in ext_acts.drain(..) {
                        act_map
                            .entry(orig_shared_id)
                            .or_insert_with(|| ExternalState {
                                name: td.shared[orig_shared_id].name.clone(),
                                width: td.shared[orig_shared_id].width,
                                acts: vec![],
                            })
                            .acts
                            .push((new_shared_id, act_id));
                    }
                    act_id += 1;
                }
                let shared_id: StateId = helper.add(&td.shared[r.shared_id]);
                pulls.push(StatePull {
                    shared_id,
                    local_id: r.local_id,
                });
                if td.shared[r.shared_id].is_ext {
                    ext_acts.push((r.shared_id, shared_id));
                }
                for (target_wth, wlocal) in dep.states[r.shared_id].write.iter() {
                    if target_wth.index() == wth.index() {
                        pushes.push(StatePush {
                            local_id: *wlocal,
                            shared_id,
                        });
                        assert!(
                            td.parts[target_wth.index()].states[*wlocal]
                                .mirror
                                .is_some()
                        );
                        assert_eq!(
                            td.shared[r.shared_id].dims,
                            td.parts[target_wth.index()].states[*wlocal].dims
                        );
                    }
                }
            }
            if pulls.len() > 0 || pushes.len() > 0 {
                pull_tasks.push(PullTask {
                    act_id,
                    pulls: std::mem::take(&mut pulls),
                });
                push_tasks.push(PushTask {
                    act_id,
                    pushes: std::mem::take(&mut pushes),
                });
                for (orig_shared_id, new_shared_id) in ext_acts.drain(..) {
                    act_map
                        .entry(orig_shared_id)
                        .or_insert_with(|| ExternalState {
                            name: td.shared[orig_shared_id].name.clone(),
                            width: td.shared[orig_shared_id].width,
                            acts: vec![],
                        })
                        .acts
                        .push((new_shared_id, act_id));
                }
                act_id += 1;
            }
            act_id = act_id.next_multiple_of(64 * 8);
            helper.add_align(64);
            td.parts[th.index()].pulls.extend(pull_tasks);
            if wth.index() < u32::MAX as usize {
                for push in push_tasks {
                    let reads = push.pushes.iter().map(|p| p.local_id).collect_vec();
                    let blk = Block {
                        orig_ids: vec![],
                        always: false,
                        next: vec![],
                        reads,
                        writes: vec![],
                        insts: vec![],
                        pushes: Some(push),
                        speculative: false,
                        observer: false,
                    };
                    td.parts[wth.index()].blocks.push(blk);
                }
            }
        }
    }
    td.shared = helper.states;
    td.shared_acts = act_id as u32;
    td.ext_states = act_map.into_values().collect_vec();
}

pub fn print_shared_comm(td: &TDesign) {
    let shared = get_shared_dep(td).states;
    let mut comm_mat = vec![vec![0; td.parts.len()]; td.parts.len()];
    for (sid, s) in shared.enumerate_ids() {
        for (read, _) in s.read.iter() {
            for (write, _) in s.write.iter() {
                comm_mat[read.index()][write.index()] += (td.shared[sid].width + 7) / 8;
            }
        }
    }
    let mut table = Table::new(&(0..td.parts.len() + 1).map(|_| "{:>}").join(" "));
    let mut first_row = (0..td.parts.len()).map(|x| format!("W{}", x)).collect_vec();
    first_row.insert(0, "".to_string());
    table.add_row(Row::from_cells(first_row));
    for (row_index, row) in comm_mat.iter().enumerate() {
        let mut row_cells = row.iter().map(|x| x.to_string()).collect_vec();
        row_cells.insert(0, format!("R{}", row_index));
        table.add_row(Row::from_cells(row_cells));
    }
    info!(
        "Shared dependency:\n{}",
        table.to_string().strip_suffix("\n").unwrap()
    );
}

pub fn get_orig_block_record(td: &TDesign) -> Vec<OrigBlockRecord> {
    let mut records = vec![];
    for (part_id, part) in td.parts.enumerate_ids() {
        for (blk_id, blk) in part.blocks.enumerate_ids() {
            for orig_id in blk.orig_ids.iter() {
                records.push(OrigBlockRecord {
                    thread: part_id.index() as u32,
                    block: blk_id.index() as u32,
                    orig_block: orig_id.index() as u32,
                });
            }
        }
    }
    records
}
