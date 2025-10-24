#![allow(unused)]

use std::{borrow::Cow, ops::Range, path::Path};

use askama::Template;
use index_vec_ext::{IndexExt, index_ext};
use itertools::Itertools;

use crate::{
    cgen::{CCArgs, CGenTask, run_cgen_tasks},
    ir::*,
    pass::{ActMap, PartId, TDesign, TPart},
};

fn int_ty_impl(width: usize) -> Cow<'static, str> {
    match width {
        0..=8 => Cow::Borrowed("uint8_t"),
        9..=16 => Cow::Borrowed("uint16_t"),
        17..=32 => Cow::Borrowed("uint32_t"),
        33..=64 => Cow::Borrowed("uint64_t"),
        65..=128 => Cow::Borrowed("__uint128_t"),
        _ => Cow::Owned(format!("unsigned _BitInt({width})")),
    }
}

pub struct CGenCfg {
    pub file_inst_size: usize,
    pub act_mask_width: u32,
    pub output_dir: String,
    pub ccargs: CCArgs,
    pub prof: bool,
}

impl Default for CGenCfg {
    fn default() -> Self {
        Self {
            file_inst_size: 8192,
            act_mask_width: 32,
            output_dir: "".to_string(),
            ccargs: CCArgs::default(),
            prof: false,
        }
    }
}

pub fn partition_by_size<T>(
    data: &[T],
    size_fn: impl Fn(&T) -> usize,
    threshold: usize,
) -> Vec<Range<usize>> {
    let mut cum_size = 0;
    let mut last_pos = 0;
    let mut result = vec![];
    for (i, item) in data.iter().enumerate() {
        let size = size_fn(item);
        if cum_size + size >= threshold {
            result.push(last_pos..i);
            last_pos = i;
            cum_size = size;
        } else {
            cum_size += size;
        }
    }
    if last_pos < data.len() {
        result.push(last_pos..data.len());
    }
    result
}

struct PartCGenCtx<'ir> {
    cfg: &'ir CGenCfg,
    states: &'ir [State],
    blocks: &'ir [Block],
    resets: &'ir [Reset],
    block_groups: Vec<Range<usize>>,
    reset_groups: Vec<Range<usize>>,
    reset_states: Vec<StateId>,
    act_map: ActMap,
}

mod filters {
    use super::*;
    use std::{borrow::Cow, fmt::Debug};

    pub fn debug(item: &dyn Debug, _: &dyn askama::Values) -> askama::Result<String> {
        Ok(format!("{:?}", item))
    }

    pub fn int_ty(width: &usize, _: &dyn askama::Values) -> askama::Result<Cow<'static, str>> {
        Ok(int_ty_impl(*width))
    }
}

impl<'ir> PartCGenCtx<'ir> {
    fn new(cfg: &'ir CGenCfg, part: &'ir TPart) -> Self {
        Self {
            cfg,
            states: &part.states,
            blocks: &part.blocks,
            resets: &part.resets,
            reset_states: part
                .resets
                .iter()
                .map(|reset| reset.reset)
                .sorted()
                .dedup()
                .collect(),
            block_groups: partition_by_size(
                &part.blocks,
                |block| block.insts.len(),
                cfg.file_inst_size,
            ),
            reset_groups: partition_by_size(
                &part.resets,
                |reset| reset.insts.len(),
                cfg.file_inst_size,
            ),
            act_map: part.get_act_map(),
        }
    }
    fn name(&self, sid: &StateId) -> &str {
        &self.states[*sid].name
    }
    fn int_ty(&self, sid: &StateId) -> Cow<'static, str> {
        int_ty_impl(self.states[*sid].width)
    }
    fn is_array(&self, sid: &StateId) -> bool {
        self.states[*sid].is_array()
    }
    fn is_small_array(&self, sid: &StateId) -> bool {
        self.states[*sid].is_array() && self.states[*sid].element_count() < 64
    }
    fn iter_ext_state(&self) -> impl Iterator<Item = StateId> {
        self.states
            .enumerate_ids()
            .filter_map(|(id, state)| (state.is_ext && state.dims.is_empty()).then_some(id))
    }
    fn iter_ext_mem(&self) -> impl Iterator<Item = StateId> {
        self.states
            .enumerate_ids()
            .filter_map(|(id, state)| (state.is_ext && !state.dims.is_empty()).then_some(id))
    }
    fn iter_block_group(&self, group_id: usize) -> impl Iterator<Item = (BlockId, &Block)> {
        let start = self.block_groups[group_id].start;
        self.blocks[self.block_groups[group_id].clone()]
            .iter()
            .enumerate()
            .map(move |(i, block)| (BlockId((i + start) as u32), block))
    }
    fn iter_reset_group(&self, group_id: usize) -> impl Iterator<Item = (BlockId, &Reset)> {
        let start = self.reset_groups[group_id].start;
        self.resets[self.reset_groups[group_id].clone()]
            .iter()
            .enumerate()
            .map(move |(i, reset)| (BlockId((i + start) as u32), reset))
    }
    fn iter_block_act_group(
        &self,
        group_id: usize,
    ) -> impl Iterator<Item = (usize, impl Iterator<Item = (BlockId, &Block)>)> {
        let range = self.block_groups[group_id].clone();
        let mut last_pos = range.start;
        std::iter::from_fn(move || {
            if last_pos >= range.end {
                return None;
            }
            let last_pos_group = last_pos / self.cfg.act_mask_width as usize;
            let next_pos = ((last_pos_group + 1) * self.cfg.act_mask_width as usize).min(range.end);
            let iter = (last_pos..next_pos).map(|i| (BlockId(i as u32), &self.blocks[i]));
            last_pos = next_pos;
            Some((last_pos_group, iter))
        })
    }
    fn act_map(&self, sid: &StateId) -> &[BlockId] {
        match self.act_map.get(sid) {
            Some(act_map) => act_map,
            None => &[],
        }
    }
}

pub struct SourceMap<'ir> {
    pub output_dir: &'ir str,
    pub decl_h: String,
    pub impl_cc: String,
    pub block_pfx: String,
    pub reset_pfx: String,
    pub makefile: String,
    pub extra_cpp: &'ir [String],
}

impl<'ir> SourceMap<'ir> {
    fn new(output_dir: &'ir str, name: &'ir str, extra_cpp: &'ir [String]) -> Self {
        Self {
            output_dir,
            decl_h: format!("{}.h", name),
            impl_cc: format!("{}.cc", name),
            block_pfx: format!("{}_blk_", name),
            reset_pfx: format!("{}_rst_", name),
            makefile: format!("Makefile"),
            extra_cpp,
        }
    }
    fn decl_h_abs(&self) -> String {
        format!("{}/{}", self.output_dir, self.decl_h)
    }
    fn impl_cc_abs(&self) -> String {
        format!("{}/{}", self.output_dir, self.impl_cc)
    }
    fn block_pfx_abs(&self) -> String {
        format!("{}/{}", self.output_dir, self.block_pfx)
    }
    fn reset_pfx_abs(&self) -> String {
        format!("{}/{}", self.output_dir, self.reset_pfx)
    }
    fn makefile_abs(&self) -> String {
        format!("{}/{}", self.output_dir, self.makefile)
    }
    fn block_group(&self, part_id: PartId, id: usize) -> String {
        format!("{}{}_{}.cc", self.block_pfx, part_id.0, id)
    }
    fn block_group_abs(&self, part_id: PartId, id: usize) -> String {
        format!("{}/{}", self.output_dir, self.block_group(part_id, id))
    }
    fn reset_group(&self, part_id: PartId, id: usize) -> String {
        format!("{}{}_{}.cc", self.reset_pfx, part_id.0, id)
    }
    fn reset_group_abs(&self, part_id: PartId, id: usize) -> String {
        format!("{}/{}", self.output_dir, self.reset_group(part_id, id))
    }
    fn extra_obj(&self, name: &str) -> String {
        use sha1::{Digest, Sha1};
        let mut hasher = Sha1::new();
        hasher.update(name.as_bytes());
        let result = hasher.finalize();
        let hex = format!("{:x}", result);
        let file_name = match Path::new(name).file_stem() {
            Some(name) => &name.to_string_lossy(),
            None => "unknown",
        };
        format!("{}.{}", file_name, &hex[..8])
    }
    fn extra_obj_abs(&self, name: &str) -> String {
        format!("{}/{}", self.output_dir, self.extra_obj(name))
    }
}

struct LargeMem<'ir> {
    pub part_id: PartId,
    pub state_id: StateId,
    pub state: &'ir State,
}

struct MTCGenCtx<'ir> {
    design: &'ir TDesign,
    cfg: &'ir CGenCfg,
    name: &'ir str,
    max_concat: u32,
    ext_fns: &'ir [String],
    shared: &'ir [State],
    large_mem: Vec<LargeMem<'ir>>,
    parts: Vec<PartCGenCtx<'ir>>,
    smap: SourceMap<'ir>,
}

index_ext! {containers PartId, PartCGenCtx<'ir>}

impl<'ir> MTCGenCtx<'ir> {
    fn new(cfg: &'ir CGenCfg, td: &'ir TDesign) -> Self {
        let mut parts = vec![];
        for part in td.parts.iter() {
            parts.push(PartCGenCtx::new(cfg, part));
        }
        let mut large_mem = vec![];
        for (part_id, part) in td.parts.enumerate_ids() {
            for (state_id, state) in part.states.enumerate_ids() {
                if state.element_count() > 1048576 {
                    large_mem.push(LargeMem {
                        part_id,
                        state_id,
                        state
                    });
                }
            }
        }
        Self {
            design: td,
            cfg,
            name: &td.name,
            max_concat: td.max_concat,
            ext_fns: &td.ext_fns,
            shared: &td.shared,
            parts,
            large_mem,
            smap: SourceMap::new(&cfg.output_dir, &td.name, &cfg.ccargs.extra_cpp),
        }
    }
    fn iter_mirrors(
        &self,
        part_id: &PartId,
    ) -> impl Iterator<Item = (StateId, AccessType, &State)> {
        self.parts[*part_id]
            .states
            .enumerate_ids()
            .filter_map(|(id, state)| match &state.mirror {
                Some(mirror) => Some((id, mirror.ty, &self.shared[mirror.shared_id])),
                None => None,
            })
    }
    fn iter_all_group_files(&'ir self) -> impl 'ir + Iterator<Item = String> {
        self.parts.enumerate_ids().flat_map(move |(part_id, part)| {
            let blks = (0..part.block_groups.len())
                .map(move |group_id| self.smap.block_group(part_id, group_id));
            let rsts = (0..part.reset_groups.len())
                .map(move |group_id| self.smap.reset_group(part_id, group_id));
            blks.chain(rsts)
        })
    }
}

#[derive(Template)]
#[template(escape = "none", path = "mt/block.cpp.j2")]
struct MTBlockGen<'ir> {
    ctx: &'ir MTCGenCtx<'ir>,
    part_id: PartId,
    group_id: usize,
}

#[derive(Template)]
#[template(escape = "none", path = "mt/reset.cpp.j2")]
struct MTResetGen<'ir> {
    ctx: &'ir MTCGenCtx<'ir>,
    part_id: PartId,
    group_id: usize,
}

#[derive(Template)]
#[template(escape = "none", path = "mt/decl.h.j2")]
struct MTDeclHGen<'ir> {
    ctx: &'ir MTCGenCtx<'ir>,
}

#[derive(Template)]
#[template(escape = "none", path = "mt/impl.cpp.j2")]
struct MTImplCGen<'ir> {
    ctx: &'ir MTCGenCtx<'ir>,
}

#[derive(Template)]
#[template(escape = "none", path = "common/makefile.j2")]
struct MakefileGen<'ir> {
    ctx: &'ir MTCGenCtx<'ir>,
}

pub fn mt_gen(d: &TDesign, cfg: &CGenCfg) -> std::io::Result<()> {
    let ctx = MTCGenCtx::new(cfg, d);
    let map = &ctx.smap;
    let mut tasks = vec![];
    for (part_id, part) in ctx.parts.enumerate_ids() {
        for group in 0..part.block_groups.len() {
            tasks.push(CGenTask {
                task: Box::new(MTBlockGen {
                    ctx: &ctx,
                    part_id,
                    group_id: group,
                }),
                file: map.block_group_abs(part_id, group),
            });
        }
        for group in 0..part.reset_groups.len() {
            tasks.push(CGenTask {
                task: Box::new(MTResetGen {
                    ctx: &ctx,
                    part_id,
                    group_id: group,
                }),
                file: map.reset_group_abs(part_id, group),
            });
        }
    }
    tasks.push(CGenTask {
        task: Box::new(MTDeclHGen { ctx: &ctx }),
        file: map.decl_h_abs(),
    });
    tasks.push(CGenTask {
        task: Box::new(MTImplCGen { ctx: &ctx }),
        file: map.impl_cc_abs(),
    });
    tasks.push(CGenTask {
        task: Box::new(MakefileGen { ctx: &ctx }),
        file: map.makefile_abs(),
    });
    run_cgen_tasks(tasks)
}
