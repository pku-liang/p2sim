use std::{fs::File, io::BufWriter};

use askama::Template;
use rayon::iter::{IntoParallelIterator, ParallelIterator};

use crate::ir::*;

pub struct CGenCfg {
    pub act_pack: u32,
    pub split_size: usize,
}

#[derive(Template)]
#[template(path = "cgen-logic.j2", escape = "none")]
struct LogicTask<'ir> {
    design_name: &'ir str,
    out_file: String,
    cfg: &'ir CGenCfg,
    logics: &'ir [Logic],
    resets: &'ir [ResetLogic],
}

mod filters {
    use std::borrow::Cow;

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
}

fn cgen_logics(d: &Design, cfg: &CGenCfg, out_dir: &str) -> std::io::Result<()> {
    let mut tasks = vec![];
    let mut cum_insts = 0;
    let mut last_pos = 0;
    let mut emit_idx = 0;
    let new_cgen_task = |out_file, logics, resets| LogicTask {
        design_name: &d.name,
        cfg,
        out_file,
        logics,
        resets,
    };
    for (i, logic) in d.logics.iter().enumerate() {
        if cum_insts + logic.insts.len() > cfg.split_size {
            tasks.push(new_cgen_task(
                format!("{out_dir}/logic_{emit_idx}.cc"),
                &d.logics[last_pos..i],
                &[],
            ));
            last_pos = i;
            cum_insts = logic.insts.len();
            emit_idx += 1;
        }
    }
    tasks.push(new_cgen_task(
        format!("{out_dir}/logic_{emit_idx}.cc"),
        &d.logics[last_pos..],
        &[],
    ));
    tasks.push(new_cgen_task(
        format!("{out_dir}/reset.cc"),
        &[],
        &d.resets[..],
    ));
    tasks
        .into_par_iter()
        .map(|task| -> std::io::Result<()> {
            let f = File::create(&task.out_file)?;
            task.write_into(&mut BufWriter::new(f))
        })
        .collect()
}

macro_rules! import_template {
    ($vis:vis $name:ident, $file:expr) => {
        #[derive(Template)]
        #[template(path = $file, escape = "none")]
        #[allow(dead_code)]
        $vis struct $name<'ir> {
            cfg: &'ir CGenCfg,
            design: &'ir Design,
        }
        impl<'ir> $name<'ir> {
            $vis fn cgen(out_file: &str, cfg: &'ir CGenCfg, design: &'ir Design) -> std::io::Result<()> {
                let f = File::create(out_file)?;
                Self { cfg, design }.write_into(&mut BufWriter::new(f))
            }
        }
    };
}

import_template!(StructDeclH, "cgen-struct-decl-h.j2");
import_template!(StructImplH, "cgen-struct-impl-h.j2");
import_template!(StructImplC, "cgen-struct-impl-cpp.j2");

fn cgen_driver(d: &Design, cfg: &CGenCfg, out_dir: &str) -> std::io::Result<()> {
    StructDeclH::cgen(&format!("{out_dir}/S{}.h", d.name), cfg, d)?;
    StructImplH::cgen(&format!("{out_dir}/S{}Impl.h", d.name), cfg, d)?;
    StructImplC::cgen(&format!("{out_dir}/S{}Impl.cc", d.name), cfg, d)?;
    Ok(())
}

pub fn cgen(d: &Design, cfg: &CGenCfg, out_dir: &str) -> std::io::Result<()> {
    if !std::fs::exists(out_dir)? {
        std::fs::create_dir_all(out_dir)?;
    }
    cgen_driver(d, cfg, out_dir)?;
    cgen_logics(d, cfg, out_dir)?;
    Ok(())
}
