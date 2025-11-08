#![allow(unused)]

use std::{borrow::Cow, fs::File, ops::Range, path::Path};

use askama::Template;
use index_vec_ext::{IndexExt, index_ext};
use itertools::Itertools;
use rustc_hash::FxHashMap;

use crate::{
    act::{BincodeExt, BlockIdMap},
    actv2::{MH_VER, MHVer, OrigBlockRecord},
    cgen::{CCArgs, CGenTask, run_cgen_tasks},
    ir::*,
    pass::{ActMap, ExternalState, PartId, TDesign, compute_shared_layout, get_orig_block_record},
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

#[derive(Default)]
pub struct ProfCfg {
    pub act_cnt: bool,
    pub rdtsc_ticks: bool,
    pub minhash: bool,
}

pub struct CGenCfg {
    pub file_inst_size: usize,
    pub act_mask_width: u32,
    pub output_dir: String,
    pub ccargs: CCArgs,
    pub prof: ProfCfg,
}

impl Default for CGenCfg {
    fn default() -> Self {
        Self {
            file_inst_size: 8192,
            act_mask_width: 32,
            output_dir: "".to_string(),
            ccargs: CCArgs::default(),
            prof: ProfCfg::default(),
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
    pulls: &'ir [PullTask],
    block_groups: Vec<Range<usize>>,
    reset_groups: Vec<Range<usize>>,
    reset_states: Vec<StateId>,
    push_range: Range<usize>,
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

pub fn iter_group<Item>(
    iter: &[Item],
    range: Range<usize>,
    group_fn: impl Fn(usize, &Item) -> usize,
) -> impl Iterator<Item = (usize, Range<usize>)> {
    let mut ptr = range.start;
    std::iter::from_fn(move || {
        if ptr >= range.end {
            return None;
        }
        let group_id = group_fn(ptr, &iter[ptr]);
        let mut next_ptr = ptr;
        loop {
            if next_ptr >= range.end {
                break;
            }
            if group_fn(next_ptr, &iter[next_ptr]) != group_id {
                break;
            }
            next_ptr += 1;
        }
        let range = ptr..next_ptr;
        ptr = next_ptr;
        Some((group_id, range))
    })
}

fn get_push_range(part: &TPart) -> Range<usize> {
    let mut first = usize::MAX;
    let mut last = usize::MIN;
    for (i, blk) in part.blocks.iter().enumerate() {
        if blk.pushes.is_some() {
            first = first.min(i);
            last = last.max(i + 1);
        }
    }
    if first == usize::MAX {
        return 0..0;
    }
    for blk in part.blocks[first..last].iter() {
        assert!(blk.pushes.is_some());
    }
    first..last
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
            pulls: &part.pulls,
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
            push_range: get_push_range(part),
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
        iter_group(self.blocks, range, |idx, _| {
            idx / self.cfg.act_mask_width as usize
        })
        .map(move |(group_id, range)| {
            (
                group_id,
                range.map(|i| (BlockId(i as u32), &self.blocks[i])),
            )
        })
    }
    fn iter_pull_act(
        &self,
    ) -> impl Iterator<Item = (usize, impl Iterator<Item = (BlockId, &PullTask)>)> {
        iter_group(self.pulls, 0..self.pulls.len(), |_, pull| {
            pull.act_id / self.cfg.act_mask_width as usize
        })
        .map(move |(group_id, range)| {
            (group_id, range.map(|i| (BlockId(i as u32), &self.pulls[i])))
        })
    }
    fn iter_push(
        &self,
    ) -> impl Iterator<Item = (usize, impl Iterator<Item = (BlockId, &PushTask)>)> {
        iter_group(self.blocks, self.push_range.clone(), |idx, _| {
            idx / self.cfg.act_mask_width as usize
        })
        .map(move |(group_id, range)| {
            (
                group_id,
                range.map(|i| (BlockId(i as u32), self.blocks[i].pushes.as_ref().unwrap())),
            )
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
    pub name: &'ir str,
    pub extra_cpp: &'ir [String],
}

macro_rules! def_src_files {
    ($($name:ident($(&self)?$(,)?$($param:ident : $type:ty),*) => ($($body:tt)*)),*$(,)?)=> {
        $(
        fn $name(&self, $($param : $type),*) -> String {
            format!($($body)*)
        }
        paste::paste! {
            fn [<$name _abs>](&self, $($param : $type),*) -> String {
                format!("{}/src/{}", self.output_dir, self.$name($($param),*))
            }
        }
        )*
    }
}

impl<'ir> SourceMap<'ir> {
    fn new(output_dir: &'ir str, name: &'ir str, extra_cpp: &'ir [String]) -> Self {
        Self {
            output_dir,
            name: name,
            extra_cpp,
        }
    }
    def_src_files! {
        thread_h(part_id: PartId) => ("thread_{}.h", part_id.0),
        thread_cc(part_id: PartId) => ("thread_{}.cc", part_id.0),
        shared_h() => ("shared.h"),
        block_group(part_id: PartId, id: usize) => ("blk_{}_{}.cc", part_id.0, id),
        reset_group(part_id: PartId, id: usize) => ("rst_{}_{}.cc", part_id.0, id),
        profdata() => ("profdata.cc"),
        minhash_h() => ("minhash.h"),
        prof_h() => ("prof.h"),
    }
    fn makefile(&self) -> String {
        "Makefile".to_string()
    }
    fn makefile_abs(&self) -> String {
        format!("{}/Makefile", self.output_dir)
    }
    fn decl_h(&self) -> String {
        format!("{}.h", self.name)
    }
    fn decl_h_abs(&self) -> String {
        format!("{}/src/{}", self.output_dir, self.decl_h())
    }
    fn impl_cc(&self) -> String {
        format!("{}.cc", self.name)
    }
    fn impl_cc_abs(&self) -> String {
        format!("{}/src/{}", self.output_dir, self.impl_cc())
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
    shared_acts: u32,
    ext_states: &'ir [ExternalState],
    large_mem: Vec<LargeMem<'ir>>,
    parts: Vec<PartCGenCtx<'ir>>,
    smap: SourceMap<'ir>,
    mh_ver: MHVer,
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
                        state,
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
            shared_acts: td.shared_acts,
            ext_states: &td.ext_states,
            parts,
            large_mem,
            smap: SourceMap::new(&cfg.output_dir, &td.name, &cfg.ccargs.extra_cpp),
            mh_ver: MH_VER,
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
        let srcs = self.parts.enumerate_ids().flat_map(move |(part_id, part)| {
            let th_cc = std::iter::once(self.smap.thread_cc(part_id));
            let blks = (0..part.block_groups.len())
                .map(move |group_id| self.smap.block_group(part_id, group_id));
            th_cc.chain(blks)
        });
        let rsts = self.parts.enumerate_ids().flat_map(move |(part_id, part)| {
            (0..part.reset_groups.len())
                .map(move |group_id| self.smap.reset_group(part_id, group_id))
        });
        let profdata = std::iter::once(self.smap.profdata());
        srcs.chain(rsts).chain(profdata)
    }
    fn name(&self, sid: &StateId) -> &str {
        &self.shared[*sid].name
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
#[template(escape = "none", path = "mt/thread.cpp.j2")]
struct MTThreadCGen<'ir> {
    ctx: &'ir MTCGenCtx<'ir>,
    part_id: PartId,
}

#[derive(Template)]
#[template(escape = "none", path = "mt/thread.h.j2")]
struct MTThreadHGen<'ir> {
    ctx: &'ir MTCGenCtx<'ir>,
    part_id: PartId,
}

#[derive(Template)]
#[template(escape = "none", path = "common/makefile.j2")]
struct MakefileGen<'ir> {
    ctx: &'ir MTCGenCtx<'ir>,
}

#[derive(Template)]
#[template(escape = "none", path = "mt/profdata.cpp.j2")]
struct MTProfDataGen<'ir> {
    ctx: &'ir MTCGenCtx<'ir>,
    records: &'ir [OrigBlockRecord],
}

#[derive(Template)]
#[template(escape = "none", path = "mt/minhash.h.j2")]
struct MTMinHashHGen<'ir> {
    ctx: &'ir MTCGenCtx<'ir>,
}

#[derive(Template)]
#[template(escape = "none", path = "mt/shared.h.j2")]
struct MTSharedHGen<'ir> {
    ctx: &'ir MTCGenCtx<'ir>,
}

#[derive(Template)]
#[template(escape = "none", path = "mt/prof.h.j2")]
struct MTProfHGen<'ir> {
    ctx: &'ir MTCGenCtx<'ir>,
}

pub fn mt_gen(d: &TDesign, cfg: &CGenCfg) -> std::io::Result<()> {
    let ctx = MTCGenCtx::new(cfg, d);
    let map = &ctx.smap;
    let records = get_orig_block_record(d);
    let mut tasks = vec![];
    std::fs::create_dir_all(format!("{}/src", cfg.output_dir))?;
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
        tasks.push(CGenTask {
            task: Box::new(MTThreadCGen { ctx: &ctx, part_id }),
            file: map.thread_cc_abs(part_id),
        });
        tasks.push(CGenTask {
            task: Box::new(MTThreadHGen { ctx: &ctx, part_id }),
            file: map.thread_h_abs(part_id),
        });
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
    tasks.push(CGenTask {
        task: Box::new(MTProfDataGen {
            ctx: &ctx,
            records: &records,
        }),
        file: map.profdata_abs(),
    });
    tasks.push(CGenTask {
        task: Box::new(MTMinHashHGen { ctx: &ctx }),
        file: map.minhash_h_abs(),
    });
    tasks.push(CGenTask {
        task: Box::new(MTSharedHGen { ctx: &ctx }),
        file: map.shared_h_abs(),
    });
    tasks.push(CGenTask {
        task: Box::new(MTProfHGen { ctx: &ctx }),
        file: map.prof_h_abs(),
    });
    run_cgen_tasks(tasks).unwrap();
    Ok(())
}
