#![allow(unused)]

use std::borrow::Cow;

use clap::{Args, Parser, ValueEnum};
use ggsim::{
    act::{ProfData, compute_block_act_map}, comp::parse_file, ir::BlockId, mt::ProfCfg, pass
};
use itertools::Itertools;
use log::info;
use rustc_hash::FxHashMap;
use serde::{Deserialize, Serialize};

#[derive(Args)]
struct ReprCCArgs {
    #[arg(short = 'I')]
    inc_dirs: Vec<String>,
    #[arg(short = 'L')]
    link_dirs: Vec<String>,
    #[arg(short = 'l')]
    link_libs: Vec<String>,
    #[arg(short = 'g')]
    debug: bool,
    #[arg(short = 'D')]
    defines: Vec<String>,
}

#[derive(Args, Default)]
struct ProfArgs {
    #[arg(long)]
    prof_act_cnt: bool,
    #[arg(long)]
    prof_rdtsc_ticks: bool,
    #[arg(long)]
    prof_rdtsc_avg: bool,
    #[arg(long)]
    prof_minhash: bool,
}

#[derive(Parser)]
struct Cli {
    inputs: Vec<String>,
    #[command(flatten)]
    repr_cc: ReprCCArgs,
    #[arg(long)]
    pgo: bool,
    #[arg(long)]
    prof_data: Option<String>,
    #[arg(short, long)]
    output: Option<String>,
    #[arg(long)]
    build: bool,
    #[arg(long)]
    dump_stats: Option<String>,
    #[arg(long)]
    diff_test: bool,
    #[command(flatten)]
    prof: ProfArgs,
    #[arg(long)]
    act: Option<String>,
    #[arg(short)]
    k: Option<usize>,
}

struct InputFiles {
    sexpr: Vec<String>,
    cpp: Vec<String>,
}

fn legalize_define(define: &str) -> String {
    if define.contains('=') {
        let split = define.split('=').collect_vec();
        assert!(split.len() == 2, "Invalid define: {}", define);
        format!("{}='{}'", split[0], split[1])
    } else {
        define.to_string()
    }
}

fn parse_input_files(files: &[impl AsRef<str>]) -> InputFiles {
    let cpp_suffixes = [".cpp", ".cc", ".c"];
    let sexpr_suffixes = [".sexp", ".sexpr", ".lexpr"];
    let has_suffix =
        |file: &str, suffixes: &[&str]| suffixes.iter().any(|suffix| file.ends_with(suffix));
    let mut sexpr = vec![];
    let mut cpp = vec![];
    for file in files {
        if has_suffix(file.as_ref(), &cpp_suffixes) {
            cpp.push(file.as_ref().to_string());
        } else if has_suffix(file.as_ref(), &sexpr_suffixes) {
            sexpr.push(file.as_ref().to_string());
        }
    }
    InputFiles { sexpr, cpp }
}

fn main() {
    env_logger::init_from_env(env_logger::Env::default().default_filter_or("info"));
    let args = Cli::parse();

    let inputs = parse_input_files(&args.inputs);
    assert!(inputs.sexpr.len() == 1, "Expect exactly one sexpr file");
    let input_sexpr = &inputs.sexpr[0];

    info!("Parsing design from {}", input_sexpr);
    let mut design = parse_file(input_sexpr).unwrap();
    info!(
        "Design stats: blocks={}, states={}, resets={}",
        design.part.blocks.len(),
        design.part.states.len(),
        design.part.resets.len()
    );
    pass::leaglize_empty_block(&mut design.part);
    pass::verify_act_map(&design.part);
    let mut pd = if args.pgo {
        let profdata = args.prof_data.unwrap();
        let prof = ProfData::from_file(&profdata).unwrap();
        let blk_act_map = compute_block_act_map(&prof);
        pass::partition(&design, args.k.unwrap_or(2), |x| (blk_act_map[x.0 as usize] * 10000.0) as i32)
    } else {
        pass::partition(&design, args.k.unwrap_or(2), |x| 0)
    };
    pass::print_part_stats(&pd);
    let mut td = pass::build_tdesign(&pd);
    pass::linear_coarsen(&mut td);
    pass::print_shared_comm(&td);
    pass::compute_shared_layout(&mut td);

    if let Some(output) = args.output {
        let to_abs_path = |path: &str| {
            std::path::absolute(path)
                .unwrap()
                .to_string_lossy()
                .to_string()
        };
        let to_abs_path_vec =
            |paths: &[String]| paths.iter().map(|s| to_abs_path(&s)).collect_vec();
        let output_dir_abs = to_abs_path(&output);

        if !std::fs::exists(&output_dir_abs).unwrap() {
            std::fs::create_dir_all(&output_dir_abs).unwrap();
        }

        use ggsim::{
            cgen::CCArgs,
            mt::{CGenCfg, mt_gen},
        };
        let cfg = CGenCfg {
            output_dir: output_dir_abs.clone(),
            ccargs: CCArgs {
                extra_cpp: to_abs_path_vec(&inputs.cpp),
                inc_dirs: to_abs_path_vec(&args.repr_cc.inc_dirs),
                link_dirs: to_abs_path_vec(&args.repr_cc.link_dirs),
                link_libs: args.repr_cc.link_libs,
                defines: args
                    .repr_cc
                    .defines
                    .iter()
                    .map(|d| legalize_define(d))
                    .collect_vec(),
                debug: args.repr_cc.debug,
            },
            prof: ProfCfg {
                act_cnt: args.prof.prof_act_cnt,
                rdtsc_ticks: args.prof.prof_rdtsc_ticks,
                rdtsc_avg: args.prof.prof_rdtsc_avg,
                minhash: args.prof.prof_minhash,
            },
            pgo: args.pgo,
            ..Default::default()
        };
        mt_gen(&td, &cfg).unwrap();

        if args.build {
            let num_threads = rayon::current_num_threads();
            let status = std::process::Command::new("make")
                .arg("-C")
                .arg(&output_dir_abs)
                .arg("-j")
                .arg(num_threads.to_string())
                .stdout(std::process::Stdio::inherit())
                .stderr(std::process::Stdio::inherit())
                .stdin(std::process::Stdio::null())
                .status()
                .unwrap();
            if status.code() != Some(0) {
                panic!("Failed to build {}", design.name);
            }
        }
    }
}
