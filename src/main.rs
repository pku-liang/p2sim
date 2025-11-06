#![allow(unused)]

use std::borrow::Cow;

use clap::{Args, Parser, ValueEnum};
use ggsim::{
    act::{ActInfo, BincodeExt, BlockIdMap, ProfData, get_block_act},
    comp::parse_file,
    ir::BlockId,
    pass,
};
use itertools::Itertools;
use log::info;
use rustc_hash::FxHashMap;
use serde::{Deserialize, Serialize};

// #[derive(ValueEnum, Clone, Copy, PartialEq, Eq)]
// enum CompileProfile {
//     #[clap(alias = "ed")]
//     EventDriven,
//     #[clap(alias = "fc")]
//     FullCycle,
//     #[clap(alias = "mt")]
//     MultiThreaded,
// }

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
}

#[derive(Args)]
struct PGOArgs {
    #[arg(long)]
    pgo: bool,
    #[arg(long)]
    pgo_id_map: Option<String>,
    #[arg(long)]
    pgo_prof_data: Option<String>,
}

#[derive(Parser)]
struct Cli {
    inputs: Vec<String>,
    #[command(flatten)]
    repr_cc: ReprCCArgs,
    #[command(flatten)]
    pgo_args: PGOArgs,
    // #[arg(short, long, default_value = "fc")]
    // profile: CompileProfile,
    #[arg(short, long)]
    output: Option<String>,
    #[arg(long)]
    build: bool,
    #[arg(long)]
    dump_stats: Option<String>,
    #[arg(long)]
    diff_test: bool,
    #[arg(long)]
    prof: bool,
    #[arg(long)]
    act: Option<String>,
    #[arg(short)]
    k: Option<usize>,
}

struct InputFiles {
    sexpr: Vec<String>,
    cpp: Vec<String>,
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

fn load_prof_data(pdata: &PGOArgs) -> FxHashMap<BlockId, u32> {
    info!(
        "Loading prof data from {}",
        pdata.pgo_id_map.as_ref().unwrap()
    );
    let id_map = BlockIdMap::read_file(pdata.pgo_id_map.as_ref().unwrap()).unwrap();
    info!(
        "Loading prof data from {}",
        pdata.pgo_prof_data.as_ref().unwrap()
    );
    let prof_data = ProfData::read_file(pdata.pgo_prof_data.as_ref().unwrap()).unwrap();
    get_block_act(&id_map, &prof_data)
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
    let pd;
    if args.pgo_args.pgo {
        let act_map = load_prof_data(&args.pgo_args);
        pd = pass::partition(&design, args.k.unwrap_or(2), |x| {
            *act_map.get(&x).unwrap() as i32
        });
    } else {
        pd = pass::partition(&design, args.k.unwrap_or(2), |x| 1);
    }
    pass::print_part_stats(&pd);
    let mut td = pass::build_tdesign(&pd);
    pass::linear_coarsen(&mut td);
    pass::print_shared_comm(&td);
    pass::compute_shared_layout(&mut td);
    // pass::verify_never_act(&design);
    // pass::verify_dep_info(&design);
    // if let Some(act_file) = args.act {
    //     info!("Reordering blocks using act info from {}", act_file);
    //     let act = ActInfo::from_file(act_file).unwrap();
    //     pass::reorder_blocks(&mut design, &act);
    // }
    // let pdesign = pass::partition(&design);
    // pass::print_pdesign_stats(&pdesign);
    // pass::verify_cs_states(&design);

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

        // if args.profile == CompileProfile::MultiThreaded {
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
                debug: args.repr_cc.debug,
            },
            prof: args.prof,
            ..Default::default()
        };
        mt_gen(&td, &cfg).unwrap();
        // } else {
        //     use ggsim::cgen::{self, CCArgs, CGenCfg};
        //     let cfg = CGenCfg {
        //         output_dir: output_dir_abs.clone(),
        //         ccargs: CCArgs {
        //             extra_cpp: to_abs_path_vec(&inputs.cpp),
        //             inc_dirs: to_abs_path_vec(&args.repr_cc.inc_dirs),
        //             link_dirs: to_abs_path_vec(&args.repr_cc.link_dirs),
        //             link_libs: args.repr_cc.link_libs,
        //             debug: args.repr_cc.debug,
        //         },
        //         prof: args.prof,
        //         diff_test: args.diff_test,
        //         ..Default::default()
        //     };

        //     info!("Generating code for {}", design.name);
        //     match args.profile {
        //         CompileProfile::EventDriven => cgen::event_driven_gen(&design, &cfg).unwrap(),
        //         CompileProfile::FullCycle => cgen::full_cycle_gen(&design, &cfg).unwrap(),
        //         _ => unreachable!(),
        //     }
        // }

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
