use std::ops::Deref;

use askama::Template;
use rayon::iter::{IntoParallelIterator, ParallelIterator};

use crate::TemplateExt;
use crate::ir::*;

pub struct CGenCfg {
    pub act_pack: u32,
    pub split_size: usize,
    pub count_act: bool,
}

pub struct ExtraCfg {
    pub cpp: Vec<String>,
    pub define: Vec<String>,
}

struct FileMap {
    decl_h: String,
    impl_h: String,
    impl_c: String,
    gprintf_c: String,
    makefile: String,
}

struct CGenCtx<'ir> {
    decl_name: String,
    impl_name: String,
    design: &'ir Design,
    cfg: &'ir CGenCfg,
    logics: Vec<(String, &'ir [Logic])>,
    resets: Vec<(String, &'ir [ResetLogic])>,
    file_map: FileMap,
    extra: &'ir ExtraCfg,
}

fn iter_logic_groups<'a>(data: &'a [Logic], pack: u32) -> impl Iterator<Item = (u32, &'a [Logic])> {
    let mut last_pos = 0;
    std::iter::from_fn(move || {
        if last_pos >= data.len() {
            None
        } else {
            if data[last_pos].always {
                let res: &[Logic] = &data[last_pos..last_pos + 1];
                return Some((data[last_pos].id.0 / pack, res));
            }
            let group = data[last_pos].id.0 / pack;
            let mut pos = last_pos + 1;
            while pos < data.len() && data[pos].id.0 / pack == group && !data[pos].always {
                pos += 1;
            }
            let res = &data[last_pos..pos];
            last_pos = pos;
            Some((group, res))
        }
    })
}

fn partition_by_size<'a, T>(
    data: &'a [T],
    size_fn: impl Fn(&T) -> usize,
    limit: usize,
) -> Vec<&'a [T]> {
    let mut cum_size = 0;
    let mut last_pos = 0;
    let mut result = vec![];
    for (i, item) in data.iter().enumerate() {
        let size = size_fn(item);
        if cum_size + size >= limit {
            result.push(&data[last_pos..i]);
            last_pos = i;
            cum_size = size;
        } else {
            cum_size += size;
        }
    }
    result.push(&data[last_pos..]);
    result
}

fn prepare_cgen<'ir>(design: &'ir Design, cfg: &'ir CGenCfg, extra: &'ir ExtraCfg) -> CGenCtx<'ir> {
    let par_logics = partition_by_size(&design.logics, |logic| logic.insts.len(), cfg.split_size);
    let par_resets = partition_by_size(&design.resets, |reset| reset.insts.len(), cfg.split_size);
    let logics = par_logics
        .into_iter()
        .enumerate()
        .map(|(i, logics)| (format!("logic_{}.cc", i), logics))
        .collect();
    let resets = par_resets
        .into_iter()
        .enumerate()
        .map(|(i, resets)| (format!("reset_{}.cc", i), resets))
        .collect();
    CGenCtx {
        decl_name: format!("S{}", design.name),
        impl_name: format!("S{}Impl", design.name),
        design,
        cfg,
        logics,
        resets,
        extra,
        file_map: FileMap {
            decl_h: format!("S{}.h", design.name),
            impl_h: format!("S{}Impl.h", design.name),
            impl_c: format!("S{}Impl.cc", design.name),
            gprintf_c: format!("gprintf.cc"),
            makefile: format!("Makefile"),
        },
    }
}

mod filters {
    use std::{borrow::Cow, path::Path};

    pub fn int_ty(width: &usize, _: &dyn askama::Values) -> askama::Result<Cow<'static, str>> {
        Ok(match width {
            0..=8 => Cow::Borrowed("uint8_t"),
            9..=16 => Cow::Borrowed("uint16_t"),
            17..=32 => Cow::Borrowed("uint32_t"),
            33..=64 => Cow::Borrowed("uint64_t"),
            65..=128 => Cow::Borrowed("__uint128_t"),
            _ => Cow::Owned(format!("unsigned _BitInt({width})")),
        })
    }

    pub fn hashed_name(name: &str, _: &dyn askama::Values) -> askama::Result<String> {
        use sha1::{Digest, Sha1};
        let mut hasher = Sha1::new();
        hasher.update(name.as_bytes());
        let result = hasher.finalize();
        let hex = format!("{:x}", result);
        let file_name = match Path::new(name).file_stem() {
            Some(name) => &name.to_string_lossy(),
            None => "unknown",
        };
        Ok(format!("{}.{}", file_name, &hex[..8]))
    }
}

#[derive(Template)]
#[template(path = "cgen-logic.cpp.j2", escape = "none")]
struct LogicTask<'ir> {
    ctx: &'ir CGenCtx<'ir>,
    step_id: usize,
    out_file: &'ir str,
    logics: &'ir [Logic],
    resets: &'ir [ResetLogic],
}

impl<'ir> Deref for LogicTask<'ir> {
    type Target = CGenCtx<'ir>;
    fn deref(&self) -> &Self::Target {
        self.ctx
    }
}

fn cgen_logics(ctx: &CGenCtx, out_dir: &str) -> std::io::Result<()> {
    let mut tasks = Vec::with_capacity(ctx.logics.len() + ctx.resets.len());
    for (step_id, (out_file, logics)) in ctx.logics.iter().enumerate() {
        tasks.push(LogicTask {
            ctx,
            step_id,
            out_file: out_file.as_str(),
            logics: logics,
            resets: &[],
        });
    }
    for (step_id, (out_file, resets)) in ctx.resets.iter().enumerate() {
        tasks.push(LogicTask {
            ctx,
            step_id,
            out_file: out_file.as_str(),
            logics: &[],
            resets: resets,
        });
    }
    tasks
        .into_par_iter()
        .map(|task| {
            let out_file = format!("{out_dir}/{}", task.out_file);
            task.write_into_file(&out_file)
        })
        .collect()
}

macro_rules! import_template {
    ($vis:vis $name:ident, $file:expr) => {
        #[derive(Template)]
        #[template(path = $file, escape = "none")]
        $vis struct $name<'ir>(&'ir CGenCtx<'ir>);
        impl<'ir> Deref for $name<'ir> {
            type Target = CGenCtx<'ir>;
            fn deref(&self) -> &Self::Target {
                self.0
            }
        }
        impl<'ir> $name<'ir> {
            $vis fn cgen(ctx: &'ir CGenCtx<'ir>, out_dir: &str, file: &str) -> std::io::Result<()> {
                Self(ctx).write_into_file(format!("{out_dir}/{file}"))
            }
        }
    }
}

import_template!(StructDeclH, "cgen-struct-decl.h.j2");
import_template!(StructImplH, "cgen-struct-impl.h.j2");
import_template!(StructImplC, "cgen-struct-impl.cpp.j2");
import_template!(Makefile, "cgen-makefile.mk.j2");
import_template!(Gprintf, "gprintf.j2");

pub fn cgen(d: &Design, cfg: &CGenCfg, out_dir: &str, extra: &ExtraCfg) -> std::io::Result<()> {
    if !std::fs::exists(out_dir)? {
        std::fs::create_dir_all(out_dir)?;
    }
    let ctx = prepare_cgen(d, cfg, extra);
    cgen_logics(&ctx, out_dir)?;
    StructDeclH::cgen(&ctx, out_dir, &ctx.file_map.decl_h)?;
    StructImplH::cgen(&ctx, out_dir, &ctx.file_map.impl_h)?;
    StructImplC::cgen(&ctx, out_dir, &ctx.file_map.impl_c)?;
    Makefile::cgen(&ctx, out_dir, &ctx.file_map.makefile)?;
    Gprintf::cgen(&ctx, out_dir, &ctx.file_map.gprintf_c)?;
    Ok(())
}
