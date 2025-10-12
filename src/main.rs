#![allow(unused)]

use std::borrow::Cow;

use clap::{Args, Parser};
use ggsim::{
    cgen::{self, CCArgs, CGenCfg},
    comp::parse_file,
};
use itertools::Itertools;
use log::info;

#[derive(Args)]
struct ReprCCArgs {
    #[arg(short = 'I')]
    inc_dirs: Vec<String>,
    #[arg(short = 'L')]
    link_dirs: Vec<String>,
    #[arg(short = 'l')]
    link_libs: Vec<String>,
}

#[derive(Parser)]
struct Cli {
    inputs: Vec<String>,
    #[command(flatten)]
    repr_cc: ReprCCArgs,
    #[arg(short, long)]
    output: String,
    #[arg(long)]
    build: bool,
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

fn main() {
    env_logger::init_from_env(env_logger::Env::default().default_filter_or("info"));
    let args = Cli::parse();

    let inputs = parse_input_files(&args.inputs);
    assert!(inputs.sexpr.len() == 1, "Expect exactly one sexpr file");
    let input_sexpr = &inputs.sexpr[0];

    let to_abs_path = |path: &str| {
        std::path::absolute(path)
            .unwrap()
            .to_string_lossy()
            .to_string()
    };
    let to_abs_path_vec = |paths: &[String]| paths.iter().map(|s| to_abs_path(&s)).collect_vec();
    let output_dir_abs = to_abs_path(&args.output);

    info!("Parsing design from {}", input_sexpr);
    let design = parse_file(input_sexpr).unwrap();

    if !std::fs::exists(&output_dir_abs).unwrap() {
        std::fs::create_dir_all(&output_dir_abs).unwrap();
    }

    let cfg = CGenCfg {
        file_inst_size: 8192,
        output_dir: output_dir_abs.clone(),
        ccargs: CCArgs {
            extra_cpp: to_abs_path_vec(&inputs.cpp),
            inc_dirs: to_abs_path_vec(&args.repr_cc.inc_dirs),
            link_dirs: to_abs_path_vec(&args.repr_cc.link_dirs),
            link_libs: args.repr_cc.link_libs,
        },
    };
    info!("Generating code for {}", design.name);
    cgen::full_cycle_gen(&design, &cfg).unwrap();

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
