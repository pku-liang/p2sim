use index_vec_ext::IndexExt;
use log::warn;
use petgraph::{
    graph::{DiGraph, NodeIndex, node_index},
    visit::EdgeRef,
};
use rustc_hash::{FxHashMap, FxHashSet};
use serde::Serialize;

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

pub fn verify_dep(design: &Design) {
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
                        "Actmap acts more than insts: {} {} => {}",
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
    for (blk_id, block) in design.blocks.enumerate_ids() {
        if !block.always && !all_acts.contains(&blk_id) {
            warn!("Block {:?} never acts", blk_id);
        }
    }
}
