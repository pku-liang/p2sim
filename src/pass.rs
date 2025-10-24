use std::collections::BTreeSet;

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
    pub design: &'d Design,
    pub states: Vec<StateDep>,
    pub cdep: DiGraph<(), ()>,
    pub sdep: DiGraph<(), StateId>,
}

impl<'d> DepInfo<'d> {
    pub fn is_sink(&self, blk_id: BlockId) -> bool {
        let has_seq_write= self.design.blocks[blk_id].writes.iter().any(|write| self.states[*write].is_seq());
        let has_no_comb_succ = self
            .cdep
            .neighbors_directed(blk_id.node_index(), Direction::Outgoing)
            .count()
            == 0;
        has_seq_write || has_no_comb_succ
    }
    pub fn get_sink_states(&self, blk_id: BlockId) -> Vec<StateId> {
        self.design.blocks[blk_id].writes.iter().filter_map(|write| {
            if self.states[*write].is_seq() {
                Some(*write)
            } else {
                None
            }
        }).collect_vec()
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

pub fn get_dep_info<'d>(design: &'d Design) -> DepInfo<'d> {
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
        if design.states[sid].is_ext {
            if !s.is_seq() {
                eprintln!("ext state {:?} is not seq {:?}", design.states[sid], s);
            }
            assert!(
                s.is_seq(),
                "ext state {:?} is not seq",
                design.states[sid].name
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
            panic!("state {:?} is not comb or seq", design.states[sid].name);
        }
    }
    DepInfo { design, states, cdep, sdep }
}

pub fn leaglize_empty_block(design: &mut Design) {
    for (blk_id, block) in design.blocks.enumerate_ids_mut() {
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
    pub design: &'d Design,
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
    pub fn new(design: &'d Design) -> Self {
        Self {
            design: design,
            blocks: vec![],
            sinks: vec![],
            resets: vec![],
        }
    }
}

pub fn partition<'d>(design: &'d Design, k: usize) -> PartDesign<'d> {
    info!("Start repcut partition");
    info!("1. Build transitive closure");
    let dep = get_dep_info(design);
    // let toposort = (0..design.blocks.len()).map(node_index).collect_vec();
    // let (succs, _) = tred::dag_to_toposorted_adjacency_list::<_, u32>(&dep.cdep, &toposort);
    // let (_, tclos) = tred::dag_transitive_reduction_closure::<_, u32>(&succs);

    info!("2. Find sink nodes");
    let mut sinks = vec![];
    let mut sink_size = vec![];
    let mut sink_map = vec![None; design.blocks.len()];
    let mut blk_in_sinks = vec![BTreeSet::default(); design.blocks.len()];
    for (blk_id, _) in design.blocks.enumerate_ids() {
        if dep.is_sink(blk_id) {
            let sink_id = sinks.len();
            let size = dep.get_sink_states(blk_id).len() as i32 + 1;
            sinks.push(blk_id);
            sink_size.push(size);
            sink_map[blk_id.0 as usize] = Some(sink_id);
            blk_in_sinks[blk_id.0 as usize].insert(sink_id);
        }
    }

    for blk_id in design.blocks.ids().rev() {
        let succ_iter = dep.cdep.neighbors_directed(blk_id.node_index(), Direction::Outgoing);
        let mut succs = BTreeSet::default();
        for succ in succ_iter {
            succs.extend(blk_in_sinks[succ.index()].iter().cloned());
        }
        blk_in_sinks[blk_id.0 as usize].extend(succs);
    }

    info!("3. Build Hypergraph");
    let mut builder = HPG::new(sink_size);
    let get_edges = |blk_id: BlockId| {
        blk_in_sinks[blk_id.0 as usize].iter().cloned()
    };
    for (blk_id, block) in design.blocks.enumerate_ids() {
        let weight = block.insts.len() as i32 / 2 + 1;
        builder.add_edge(weight, get_edges(blk_id));
        if block.always {
            builder.add_edge(10000, get_edges(blk_id));
        }
    }
    for (sid, s) in dep.states.enumerate_ids() {
        if design.states[sid].element_count() > 32 {
            let weight = design.states[sid].element_count() as i32;
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
            .verbose(false)
            .build()
            .unwrap();
        let hpg = builder.build(&ctx).unwrap();
        let part = hpg.partition().unwrap();
        part_id = part.extract_partition();
        assert!(part_id.len() == sinks.len());
    }

    info!("5. Extract partition");
    let mut parts = vec![Part::new(design); k];
    let mut blk_in_parts = vec![BTreeSet::default(); design.blocks.len()];

    for (i, sink) in sinks.iter().enumerate() {
        blk_in_parts[sink.0 as usize].insert(part_id[i] as usize);
        if sink.0 == 6789 {
            eprintln!("sink = {:?} part_id = {:?}", sink, part_id[i]);
        }
        parts[part_id[i] as usize].sinks.push(*sink);
    }
    for blk_id in design.blocks.ids().rev() {
        let mut insides = BTreeSet::default();
        let succ_iter = dep
            .cdep
            .neighbors_directed(blk_id.node_index(), Direction::Outgoing);
        for succ in succ_iter {
            insides.extend(blk_in_parts[succ.index()].iter().cloned());
        }
        blk_in_parts[blk_id.0 as usize].extend(insides);
    }
    for blk_id in design.blocks.ids() {
        for part_id in blk_in_parts[blk_id.0 as usize].iter() {
            parts[*part_id].blocks.push(blk_id);
        }
    }
    for (rst_id, reset) in design.resets.enumerate_ids() {
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

    let mut states = vec![StateInfo::default(); design.states.len()];
    for (part_id, part) in parts.enumerate_ids() {
        for sink in part.sinks.iter() {
            for write in &design.blocks[*sink].writes {
                states[*write].write.insert(part_id);
                states[*write].is_shared = true;
            }
        }
        for rst_id in part.resets.iter() {
            let reset = design.resets[*rst_id].reset;
            if dep.states[reset].is_seq() {
                states[reset].read.insert(part_id);
            }
        }
        for blk_id in part.blocks.iter() {
            for read in &design.blocks[*blk_id].reads {
                if dep.states[*read].is_seq() {
                    states[*read].read.insert(part_id);
                }
            }
        }
    }
    for (sid, s) in states.enumerate_ids_mut() {
        if design.states[sid].is_ext {
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
            .map(|block| pd.design.blocks[*block].insts.len())
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
    info!("Block Partition Stats:\n{}", table.to_string().strip_suffix("\n").unwrap());
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
        let first_write = pd.dep.states[*id].write.first().cloned().unwrap_or(PartId(0));
        // let first_read = pd.dep.states[*id].read.first().cloned().unwrap_or(PartId(0));
        // (first_write, first_read)
        first_write
    });
    let mut shared: Vec<State> = vec![];
    let mut shared_id_map = FxHashMap::default();
    for id in shared_ids.iter() {
        let new_id = shared.add(pd.design.states[*id].clone());
        shared_id_map.insert(*id, new_id);
    }
    let mut parts = vec![];
    for (part_id, part) in pd.parts.enumerate_ids() {
        let mut all_states = BTreeSet::<StateId>::default();
        for rst_id in part.resets.iter() {
            all_states.insert(pd.design.resets[*rst_id].reset);
        }
        for blk_id in part.blocks.iter() {
            all_states.extend(pd.design.blocks[*blk_id].reads.iter().cloned());
            all_states.extend(pd.design.blocks[*blk_id].writes.iter().cloned());
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
            let mut state = part.design.states[*sid].clone();
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
            let mut new_block = pd.design.blocks[*blk_id].clone();
            rewrite.update_block(&mut new_block);
            blocks.push(new_block);
        }
        for rst_id in part.resets.iter() {
            let mut new_reset = pd.design.resets[*rst_id].clone();
            rewrite.update_reset(&mut new_reset);
            resets.push(new_reset);
        }
        parts.push(TPart {
            states,
            blocks,
            resets,
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
        shared,
        parts,
    }
}
