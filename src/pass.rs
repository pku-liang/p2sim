use index_vec_ext::IndexExt;
use petgraph::graph::{DiGraph, NodeIndex, node_index};
use rustc_hash::FxHashMap;

use crate::ir::*;

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
                if *write_blk != blk_id {
                    sdep.add_edge(write_blk.node_index(), blk_id.node_index(), ());
                }
            }
            last_read_blks.remove(write);
            last_write_blks.remove(write);
        }
    }
    DepInfo { cdep, sdep }
}
