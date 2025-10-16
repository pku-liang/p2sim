use index_vec_ext::IndexExt;
use log::warn;
use petgraph::{graph::{node_index, DiGraph, NodeIndex}, visit::EdgeRef};
use rustc_hash::FxHashMap;
use serde::Serialize;

use crate::ir::*;

pub type ActMap = FxHashMap<StateId, Vec<BlockId>>;

pub struct DepInfo {
    /// combinational dependency
    /// (i -> j) <=> (blk[t, i] -> blk[t, j])
    pub cdep: DiGraph<(), ()>,

    /// sequential dependency
    /// (i -> j) <=> (blk[t, i] -> blk[t + 1, j])
    pub sdep: DiGraph<(), ()>,
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
    let mut cdep = DiGraph::new();
    let mut sdep = DiGraph::new();
    for _ in &design.blocks {
        cdep.add_node(());
        sdep.add_node(());
    }
    let mut last_read_blks: FxHashMap<StateId, Vec<BlockId>> = FxHashMap::default();
    let mut last_write_blks: FxHashMap<StateId, BlockId> = FxHashMap::default();
    for (blk_id, block) in design.blocks.enumerate_ids() {
        for read in &block.reads {
            // read-after-write
            if let Some(write_blk) = last_write_blks.get(read) {
                cdep.add_edge(write_blk.node_index(), blk_id.node_index(), ());
            }
            last_read_blks.entry(*read).or_default().push(blk_id);
        }
        for write in &block.writes {
            // write-after-read
            if let Some(read_blks) = last_read_blks.get(write) {
                for r in read_blks {
                    sdep.add_edge(r.node_index(), blk_id.node_index(), ());
                }
            }
            // write-after-write
            if let Some(write_blk) = last_write_blks.get(write) {
                sdep.add_edge(write_blk.node_index(), blk_id.node_index(), ());
            }
            // clear reads
            last_read_blks.remove(write);
            last_write_blks.insert(*write, blk_id);
        }
    }
    for (blk_id, block) in design.blocks.enumerate_ids() {
        for read in &block.reads {
            // read-after-write
            if let Some(write_blk) = last_write_blks.get(read) {
                cdep.add_edge(write_blk.node_index(), blk_id.node_index(), ());
            }
        }
        for write in &block.writes {
            // write-after-read
            if let Some(read_blks) = last_read_blks.get(write) {
                for r in read_blks {
                    sdep.add_edge(r.node_index(), blk_id.node_index(), ());
                }
            }
            // write-after-write
            if let Some(write_blk) = last_write_blks.get(write) {
                sdep.add_edge(write_blk.node_index(), blk_id.node_index(), ());
            }
            last_read_blks.remove(write);
            last_write_blks.remove(write);
        }
    }
    DepInfo { cdep, sdep }
}

pub fn get_act_map(design: &Design) -> ActMap {
    let mut act_map: ActMap = FxHashMap::default();
    for (blk_id, block) in design.blocks.enumerate_ids() {
        for read in &block.reads {
            act_map.entry(*read).or_default().push(blk_id);
        }
    }
    act_map
}

#[derive(Debug, Serialize)]
pub struct DepStats {
    pub num_cdep: usize,
    pub cdep_edges: Vec<(usize, usize)>,
    pub num_sdep: usize,
    pub sdep_edges: Vec<(usize, usize)>,
}

pub fn get_dep_stats(design: &Design) -> DepStats {
    let dep_info = get_dep_info(design);
    let mut stats = DepStats {
        num_cdep: dep_info.cdep.node_count(),
        cdep_edges: Vec::new(),
        num_sdep: dep_info.sdep.node_count(),
        sdep_edges: Vec::new(),
    };
    for e in dep_info.cdep.edge_references() {
        stats.cdep_edges.push((e.source().index(), e.target().index()));
    }
    for e in dep_info.sdep.edge_references() {
        stats.sdep_edges.push((e.source().index(), e.target().index()));
    }
    stats
}

pub fn verify_dep(design: &Design) {
    let act_map = get_act_map(design);
    for block in &design.blocks {
        for inst in &block.insts {
            if let Inst::WritePost { sid, acts, .. } = inst {
                let act_map_acts = act_map.get(sid).map(|x| &x[..]).unwrap_or(&[]);
                for a in acts {
                    assert!(act_map_acts.contains(a), "Act {} doesn't include {:?}, ({:?})", design.states[*sid].name, a, act_map_acts);
                }
                if acts.len() < act_map_acts.len() {
                    warn!("Actmap acts more than insts: {} {} => {}", design.states[*sid].name, acts.len(), act_map_acts.len());
                }
            } 
        }
    }
}