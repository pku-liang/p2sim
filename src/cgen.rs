// #![allow(unused)]

// use std::{
//     borrow::Cow,
//     ops::{Deref, Range},
//     path::Path,
// };

use indicatif::ParallelProgressIterator;
// use askama::Template;
// use index_vec_ext::IndexExt;
// use itertools::Itertools;
use log::info;
use rayon::iter::{IntoParallelIterator, ParallelIterator};
// use rustc_hash::FxHashMap;

// use crate::{TemplateExt, ir::*, pass::get_act_map};
use crate::TemplateExt;

// pub fn int_ty_impl(width: usize) -> Cow<'static, str> {
//     match width {
//         0..=8 => Cow::Borrowed("uint8_t"),
//         9..=16 => Cow::Borrowed("uint16_t"),
//         17..=32 => Cow::Borrowed("uint32_t"),
//         33..=64 => Cow::Borrowed("uint64_t"),
//         65..=128 => Cow::Borrowed("__uint128_t"),
//         _ => Cow::Owned(format!("unsigned _BitInt({width})")),
//     }
// }

#[derive(Debug, Default)]
pub struct CCArgs {
    pub extra_cpp: Vec<String>,
    pub inc_dirs: Vec<String>,
    pub link_dirs: Vec<String>,
    pub link_libs: Vec<String>,
    pub debug: bool,
}

// #[derive(Debug)]
// pub struct CGenCfg {
//     pub file_inst_size: usize,
//     pub output_dir: String,
//     pub ccargs: CCArgs,
//     pub diff_test: bool,
//     pub prof: bool,
//     pub act_mask_width: u32,
// }

// impl Default for CGenCfg {
//     fn default() -> Self {
//         Self {
//             file_inst_size: 8192,
//             output_dir: "".to_string(),
//             ccargs: CCArgs::default(),
//             diff_test: false,
//             prof: false,
//             act_mask_width: 32,
//         }
//     }
// }

// pub struct SourceMap<'ir> {
//     pub output_dir: &'ir str,
//     pub decl_h: String,
//     pub impl_cc: String,
//     pub block_pfx: String,
//     pub reset_pfx: String,
//     pub makefile: String,
//     pub extra_cpp: &'ir [String],
// }

// impl<'ir> SourceMap<'ir> {
//     fn new(output_dir: &'ir str, name: &'ir str, extra_cpp: &'ir [String]) -> Self {
//         Self {
//             output_dir,
//             decl_h: format!("{}.h", name),
//             impl_cc: format!("{}.cc", name),
//             block_pfx: format!("{}_blk_", name),
//             reset_pfx: format!("{}_rst_", name),
//             makefile: format!("Makefile"),
//             extra_cpp,
//         }
//     }
//     fn decl_h_abs(&self) -> String {
//         format!("{}/{}", self.output_dir, self.decl_h)
//     }
//     fn impl_cc_abs(&self) -> String {
//         format!("{}/{}", self.output_dir, self.impl_cc)
//     }
//     fn block_pfx_abs(&self) -> String {
//         format!("{}/{}", self.output_dir, self.block_pfx)
//     }
//     fn reset_pfx_abs(&self) -> String {
//         format!("{}/{}", self.output_dir, self.reset_pfx)
//     }
//     fn makefile_abs(&self) -> String {
//         format!("{}/{}", self.output_dir, self.makefile)
//     }
//     fn block_group(&self, id: usize) -> String {
//         format!("{}{}.cc", self.block_pfx, id)
//     }
//     fn block_group_abs(&self, id: usize) -> String {
//         format!("{}/{}", self.output_dir, self.block_group(id))
//     }
//     fn reset_group(&self, id: usize) -> String {
//         format!("{}{}.cc", self.reset_pfx, id)
//     }
//     fn reset_group_abs(&self, id: usize) -> String {
//         format!("{}/{}", self.output_dir, self.reset_group(id))
//     }
//     fn extra_obj(&self, name: &str) -> String {
//         use sha1::{Digest, Sha1};
//         let mut hasher = Sha1::new();
//         hasher.update(name.as_bytes());
//         let result = hasher.finalize();
//         let hex = format!("{:x}", result);
//         let file_name = match Path::new(name).file_stem() {
//             Some(name) => &name.to_string_lossy(),
//             None => "unknown",
//         };
//         format!("{}.{}", file_name, &hex[..8])
//     }
//     fn extra_obj_abs(&self, name: &str) -> String {
//         format!("{}/{}", self.output_dir, self.extra_obj(name))
//     }
// }

// struct CGenCtx<'ir> {
//     cfg: &'ir CGenCfg,
//     name: &'ir str,
//     max_concat: u32,
//     ext_fns: &'ir [String],
//     states: &'ir [State],
//     blocks: &'ir [Block],
//     resets: &'ir [Reset],
//     block_groups: Vec<Range<usize>>,
//     reset_groups: Vec<Range<usize>>,
//     smap: SourceMap<'ir>,
//     reset_states: Vec<StateId>,
// }

// mod filters {
//     use super::*;
//     use std::borrow::Cow;

//     pub fn int_ty(width: &usize, _: &dyn askama::Values) -> askama::Result<Cow<'static, str>> {
//         Ok(int_ty_impl(*width))
//     }
// }

// pub fn partition_by_size<T>(
//     data: &[T],
//     size_fn: impl Fn(&T) -> usize,
//     threshold: usize,
// ) -> Vec<Range<usize>> {
//     let mut cum_size = 0;
//     let mut last_pos = 0;
//     let mut result = vec![];
//     for (i, item) in data.iter().enumerate() {
//         let size = size_fn(item);
//         if cum_size + size >= threshold {
//             result.push(last_pos..i);
//             last_pos = i;
//             cum_size = size;
//         } else {
//             cum_size += size;
//         }
//     }
//     if last_pos < data.len() {
//         result.push(last_pos..data.len());
//     }
//     result
// }

// impl<'ir> CGenCtx<'ir> {
//     fn new(cfg: &'ir CGenCfg, design: &'ir Design) -> Self {
//         Self {
//             cfg: cfg,
//             name: &design.name,
//             max_concat: design.max_concat,
//             states: &design.part.states,
//             blocks: &design.part.blocks,
//             resets: &design.part.resets,
//             ext_fns: &design.ext_fns,
//             reset_states: design
//                 .part
//                 .resets
//                 .iter()
//                 .map(|reset| reset.reset)
//                 .sorted()
//                 .dedup()
//                 .collect(),
//             block_groups: partition_by_size(
//                 &design.part.blocks,
//                 |block| block.insts.len(),
//                 cfg.file_inst_size,
//             ),
//             reset_groups: partition_by_size(
//                 &design.part.resets,
//                 |reset| reset.insts.len(),
//                 cfg.file_inst_size,
//             ),
//             smap: SourceMap::new(&cfg.output_dir, &design.name, &cfg.ccargs.extra_cpp),
//         }
//     }
//     fn int_ty(&self, sid: &StateId) -> Cow<'static, str> {
//         int_ty_impl(self.states[*sid].width)
//     }
//     fn name(&self, sid: &StateId) -> &str {
//         &self.states[*sid].name
//     }
//     fn is_array(&self, sid: &StateId) -> bool {
//         self.states[*sid].is_array()
//     }
//     fn is_small_array(&self, sid: &StateId) -> bool {
//         self.states[*sid].is_array() && self.states[*sid].element_count() < 64
//     }
//     fn iter_ext_state(&self) -> impl Iterator<Item = StateId> {
//         self.states
//             .enumerate_ids()
//             .filter_map(|(id, state)| (state.is_ext && state.dims.is_empty()).then_some(id))
//     }
//     fn iter_ext_mem(&self) -> impl Iterator<Item = StateId> {
//         self.states
//             .enumerate_ids()
//             .filter_map(|(id, state)| (state.is_ext && !state.dims.is_empty()).then_some(id))
//     }
//     fn iter_block_group(&self, group_id: usize) -> impl Iterator<Item = (BlockId, &Block)> {
//         let start = self.block_groups[group_id].start;
//         self.blocks[self.block_groups[group_id].clone()]
//             .iter()
//             .enumerate()
//             .map(move |(i, block)| (BlockId((i + start) as u32), block))
//     }
//     fn iter_reset_group(&self, group_id: usize) -> impl Iterator<Item = (BlockId, &Reset)> {
//         let start = self.reset_groups[group_id].start;
//         self.resets[self.reset_groups[group_id].clone()]
//             .iter()
//             .enumerate()
//             .map(move |(i, reset)| (BlockId((i + start) as u32), reset))
//     }
//     fn iter_block_act_group(
//         &self,
//         group_id: usize,
//     ) -> impl Iterator<Item = (usize, impl Iterator<Item = (BlockId, &Block)>)> {
//         let range = self.block_groups[group_id].clone();
//         let mut last_pos = range.start;
//         std::iter::from_fn(move || {
//             if last_pos >= range.end {
//                 return None;
//             }
//             let last_pos_group = last_pos / self.cfg.act_mask_width as usize;
//             let next_pos = ((last_pos_group + 1) * self.cfg.act_mask_width as usize).min(range.end);
//             let iter = (last_pos..next_pos).map(|i| (BlockId(i as u32), &self.blocks[i]));
//             last_pos = next_pos;
//             Some((last_pos_group, iter))
//         })
//     }
//     fn iter_all_group_files(&'ir self) -> impl 'ir + Iterator<Item = String> {
//         (0..self.block_groups.len())
//             .map(move |group_id| self.smap.block_group(group_id))
//             .chain(
//                 (0..self.reset_groups.len()).map(move |group_id| self.smap.reset_group(group_id)),
//             )
//         // self.block_groups
//         //     .iter()
//         //     .map(move |group| self.smap.block_group(group.start))
//         //     .chain(
//         //         self.reset_groups
//         //             .iter()
//         //             .map(move |group| self.smap.reset_group(group.start)),
//         //     )
//     }
// }

// #[derive(Template)]
// #[template(escape = "none", path = "full-cycle/block.cpp.j2")]
// struct FCBlockGen<'ir> {
//     ctx: &'ir CGenCtx<'ir>,
//     group_id: usize,
// }

// #[derive(Template)]
// #[template(escape = "none", path = "full-cycle/reset.cpp.j2")]
// struct FCResetGen<'ir> {
//     ctx: &'ir CGenCtx<'ir>,
//     group_id: usize,
// }

// #[derive(Template)]
// #[template(escape = "none", path = "full-cycle/decl.h.j2")]
// struct FCDeclHGen<'ir> {
//     ctx: &'ir CGenCtx<'ir>,
// }

// #[derive(Template)]
// #[template(escape = "none", path = "full-cycle/impl.cpp.j2")]
// struct FCImplCGen<'ir> {
//     ctx: &'ir CGenCtx<'ir>,
// }

// #[derive(Template)]
// #[template(escape = "none", path = "common/makefile.j2")]
// struct MakefileGen<'ir> {
//     ctx: &'ir CGenCtx<'ir>,
// }

pub(crate) trait WriteToFileStr {
    fn write_into_file_str(&self, path: &str) -> std::io::Result<()>;
}
impl<T: TemplateExt> WriteToFileStr for T {
    fn write_into_file_str(&self, path: &str) -> std::io::Result<()> {
        TemplateExt::write_into_file(self, path)
    }
}

pub(crate) struct CGenTask<'ir> {
    pub(crate) task: Box<dyn WriteToFileStr + Send + 'ir>,
    pub(crate) file: String,
}

pub(crate) fn run_cgen_tasks(tasks: Vec<CGenTask>) -> std::io::Result<()> {
    // for task in tasks.iter() {
    //     info!("Generating {}", task.file);
    //     task.task.write_into_file_str(&task.file)?;
    // }
    // Ok(())
    info!("Running {} cgen tasks", tasks.len());
    let count = tasks.len();
    tasks
        .into_par_iter()
        .progress_count(count as u64)
        .map(|task| task.task.write_into_file_str(&task.file))
        .collect()
}

// pub fn full_cycle_gen(d: &Design, cfg: &CGenCfg) -> std::io::Result<()> {
//     let ctx = CGenCtx::new(cfg, d);
//     let map = &ctx.smap;
//     let mut tasks = vec![];
//     for group_id in 0..ctx.block_groups.len() {
//         tasks.push(CGenTask {
//             task: Box::new(FCBlockGen {
//                 ctx: &ctx,
//                 group_id,
//             }),
//             file: map.block_group_abs(group_id),
//         });
//     }
//     for group_id in 0..ctx.reset_groups.len() {
//         tasks.push(CGenTask {
//             task: Box::new(FCResetGen {
//                 ctx: &ctx,
//                 group_id,
//             }),
//             file: map.reset_group_abs(group_id),
//         });
//     }
//     tasks.push(CGenTask {
//         task: Box::new(FCDeclHGen { ctx: &ctx }),
//         file: map.decl_h_abs(),
//     });
//     tasks.push(CGenTask {
//         task: Box::new(FCImplCGen { ctx: &ctx }),
//         file: map.impl_cc_abs(),
//     });
//     tasks.push(CGenTask {
//         task: Box::new(MakefileGen { ctx: &ctx }),
//         file: map.makefile_abs(),
//     });
//     info!("Running {} cgen tasks", tasks.len());
//     run_cgen_tasks(tasks)
// }

// struct EDCGenCtx<'ir> {
//     ctx: CGenCtx<'ir>,
//     act_map: FxHashMap<StateId, Vec<BlockId>>,
// }

// impl<'ir> Deref for EDCGenCtx<'ir> {
//     type Target = CGenCtx<'ir>;
//     fn deref(&self) -> &Self::Target {
//         &self.ctx
//     }
// }

// impl<'ir> EDCGenCtx<'ir> {
//     fn new(cfg: &'ir CGenCfg, design: &'ir Design) -> Self {
//         let act_map = get_act_map(&design.part);
//         Self {
//             ctx: CGenCtx::new(cfg, design),
//             act_map,
//         }
//     }
//     fn act_map(&self, sid: &StateId) -> &[BlockId] {
//         if let Some(act_map) = self.act_map.get(sid) {
//             act_map
//         } else {
//             &[]
//         }
//     }
// }

// #[derive(Template)]
// #[template(escape = "none", path = "event-driven/block.cpp.j2")]
// struct EDBlockGen<'ir> {
//     ctx: &'ir EDCGenCtx<'ir>,
//     group_id: usize,
// }

// #[derive(Template)]
// #[template(escape = "none", path = "event-driven/reset.cpp.j2")]
// struct EDResetGen<'ir> {
//     ctx: &'ir EDCGenCtx<'ir>,
//     group_id: usize,
// }

// #[derive(Template)]
// #[template(escape = "none", path = "event-driven/decl.h.j2")]
// struct EDDeclHGen<'ir> {
//     ctx: &'ir EDCGenCtx<'ir>,
// }

// #[derive(Template)]
// #[template(escape = "none", path = "event-driven/impl.cpp.j2")]
// struct EDImplCGen<'ir> {
//     ctx: &'ir EDCGenCtx<'ir>,
// }

// pub fn event_driven_gen(d: &Design, cfg: &CGenCfg) -> std::io::Result<()> {
//     let ctx = EDCGenCtx::new(cfg, d);
//     let map = &ctx.smap;
//     let mut tasks = vec![];
//     for group_id in 0..ctx.block_groups.len() {
//         tasks.push(CGenTask {
//             task: Box::new(EDBlockGen {
//                 ctx: &ctx,
//                 group_id,
//             }),
//             file: map.block_group_abs(group_id),
//         });
//     }
//     for group_id in 0..ctx.reset_groups.len() {
//         tasks.push(CGenTask {
//             task: Box::new(EDResetGen {
//                 ctx: &ctx,
//                 group_id,
//             }),
//             file: map.reset_group_abs(group_id),
//         });
//     }
//     tasks.push(CGenTask {
//         task: Box::new(EDDeclHGen { ctx: &ctx }),
//         file: map.decl_h_abs(),
//     });
//     tasks.push(CGenTask {
//         task: Box::new(EDImplCGen { ctx: &ctx }),
//         file: map.impl_cc_abs(),
//     });
//     tasks.push(CGenTask {
//         task: Box::new(MakefileGen { ctx: &ctx }),
//         file: map.makefile_abs(),
//     });
//     run_cgen_tasks(tasks)
// }
