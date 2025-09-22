use std::{fs::File, path::{absolute, Path}};

use clap::Parser;
use ggsim::{
    cgen::{CGenCfg, cgen},
    compile::parse,
    ir::Design,
};
use memmap2::Mmap;

#[derive(Parser)]
struct Cli {
    input: Vec<String>,
    #[arg(short, long)]
    output: String,
}

struct Input {
    sexpr: Vec<String>,
    cpp: Vec<String>,
}

impl Input {
    fn new(inputs: Vec<String>) -> Self {
        Self {
            sexpr: inputs.iter().filter(|s| s.ends_with(".sexp") || s.ends_with(".sexpr")).cloned().collect(),
            cpp: inputs.iter().filter(|s| s.ends_with(".cpp") || s.ends_with(".cc")).cloned().collect(),
        }
    }
}

fn parse_file(input: &str) -> Design {
    let file = File::open(input).unwrap();
    let mmap = unsafe { Mmap::map(&file).unwrap() };
    let data = unsafe { str::from_utf8_unchecked(&mmap[..]) };
    parse(data).unwrap()
}

fn main() {
    let cli = Cli::parse();
    let input = Input::new(cli.input);
    assert!(input.sexpr.len() == 1, "only one sexpr file is supported");
    let d = parse_file(&input.sexpr[0]);
    let cfg = CGenCfg {
        act_pack: 32,
        split_size: 8192,
    };
    let mut extra = vec![];
    for cpp in input.cpp {
        extra.push(absolute(&cpp).unwrap().to_string_lossy().to_string());
    }
    cgen(&d, &cfg, &cli.output, extra).unwrap();
}
