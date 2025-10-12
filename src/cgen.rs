#![allow(unused)]

use std::{borrow::Cow, ops::Range, path::Path};

use askama::Template;
use index_vec_ext::IndexExt;
use itertools::Itertools;
use log::info;
use rayon::iter::{IntoParallelIterator, ParallelIterator};

use crate::{TemplateExt, ir::*};

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

#[derive(Debug)]
pub struct CCArgs {
    pub extra_cpp: Vec<String>,
    pub inc_dirs: Vec<String>,
    pub link_dirs: Vec<String>,
    pub link_libs: Vec<String>,
}

#[derive(Debug)]
pub struct CGenCfg {
    pub file_inst_size: usize,
    pub output_dir: String,
    pub ccargs: CCArgs,
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
    fn block_group(&self, id: usize) -> String {
        format!("{}{}.cc", self.block_pfx, id)
    }
    fn block_group_abs(&self, id: usize) -> String {
        format!("{}/{}", self.output_dir, self.block_group(id))
    }
    fn reset_group(&self, id: usize) -> String {
        format!("{}{}.cc", self.reset_pfx, id)
    }
    fn reset_group_abs(&self, id: usize) -> String {
        format!("{}/{}", self.output_dir, self.reset_group(id))
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

struct CGenCtx<'ir> {
    cfg: &'ir CGenCfg,
    name: &'ir str,
    max_concat: u32,
    ext_fns: &'ir [String],
    states: &'ir [State],
    blocks: &'ir [Block],
    resets: &'ir [Reset],
    block_groups: Vec<Range<usize>>,
    reset_groups: Vec<Range<usize>>,
    smap: SourceMap<'ir>,
}

mod filters {
    use super::*;
    use std::borrow::Cow;

    pub fn int_ty(width: &usize, _: &dyn askama::Values) -> askama::Result<Cow<'static, str>> {
        Ok(int_ty_impl(*width))
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

impl<'ir> CGenCtx<'ir> {
    fn new(cfg: &'ir CGenCfg, design: &'ir Design) -> Self {
        Self {
            cfg: cfg,
            name: &design.name,
            max_concat: design.max_concat,
            states: &design.states,
            blocks: &design.blocks,
            resets: &design.resets,
            ext_fns: &design.ext_fns,
            block_groups: partition_by_size(
                &design.blocks,
                |block| block.insts.len(),
                cfg.file_inst_size,
            ),
            reset_groups: partition_by_size(
                &design.resets,
                |reset| reset.insts.len(),
                cfg.file_inst_size,
            ),
            smap: SourceMap::new(&cfg.output_dir, &design.name, &cfg.ccargs.extra_cpp),
        }
    }
    fn int_ty(&self, sid: &StateId) -> Cow<'static, str> {
        int_ty_impl(self.states[*sid].width)
    }
    fn name(&self, sid: &StateId) -> &str {
        &self.states[*sid].name
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
        let start = self.block_groups[group_id].start;
        self.resets[self.reset_groups[group_id].clone()]
            .iter()
            .enumerate()
            .map(move |(i, reset)| (BlockId((i + start) as u32), reset))
    }
}

#[derive(Template)]
#[template(escape = "none", path = "full-cycle/block.cpp.j2")]
struct FCBlockGen<'ir> {
    ctx: &'ir CGenCtx<'ir>,
    group_id: usize,
}

#[derive(Template)]
#[template(escape = "none", path = "full-cycle/reset.cpp.j2")]
struct FCResetGen<'ir> {
    ctx: &'ir CGenCtx<'ir>,
    group_id: usize,
}

#[derive(Template)]
#[template(escape = "none", path = "full-cycle/decl.h.j2")]
struct FCDeclHGen<'ir> {
    ctx: &'ir CGenCtx<'ir>,
}

#[derive(Template)]
#[template(escape = "none", path = "full-cycle/impl.cpp.j2")]
struct FCImplCGen<'ir> {
    ctx: &'ir CGenCtx<'ir>,
}

#[derive(Template)]
#[template(escape = "none", path = "common/makefile.j2")]
struct MakefileGen<'ir> {
    ctx: &'ir CGenCtx<'ir>,
}

pub fn full_cycle_gen(d: &Design, cfg: &CGenCfg) -> std::io::Result<()> {
    let ctx = CGenCtx::new(cfg, d);
    let map = &ctx.smap;

    info!("Generate fc_block_tasks={}, fc_reset_tasks={}", ctx.block_groups.len(), ctx.reset_groups.len());
    let fc_block_tasks = (0..ctx.block_groups.len())
        .map(|group_id| FCBlockGen {
            ctx: &ctx,
            group_id,
        })
        .collect_vec();
    fc_block_tasks
        .into_par_iter()
        .map(|task| task.write_into_file(map.block_group_abs(task.group_id)))
        .collect::<std::io::Result<()>>()?;
    let fc_reset_tasks = (0..ctx.reset_groups.len())
        .map(|group_id| FCResetGen {
            ctx: &ctx,
            group_id,
        })
        .collect_vec();
    fc_reset_tasks
        .into_par_iter()
        .map(|task| task.write_into_file(map.reset_group_abs(task.group_id)))
        .collect::<std::io::Result<()>>()?;

    info!("Generate other files");
    FCDeclHGen { ctx: &ctx }.write_into_file(map.decl_h_abs())?;
    FCImplCGen { ctx: &ctx }.write_into_file(map.impl_cc_abs())?;
    MakefileGen { ctx: &ctx }.write_into_file(map.makefile_abs())?;
    Ok(())
}
