use std::{fs::File, path::absolute};

use clap::Parser;
use ggsim::{
    analysis::{dump_logic_graph_gephi, print_logic_info, GephiDumpCfg}, cgen::{cgen, CGenCfg, ExtraCfg}, compile::parse, ir::Design
};
use memmap2::Mmap;

#[derive(Parser)]
struct Cli {
    input: Vec<String>,
    #[arg(short, long)]
    output: Option<String>,
    #[arg(long)]
    dump_gephi: Option<String>,
    #[arg(short = 'D')]
    defines: Vec<String>,
}

struct Input {
    sexpr: Vec<String>,
    cpp: Vec<String>,
}

impl Input {
    fn new(inputs: Vec<String>) -> Self {
        Self {
            sexpr: inputs
                .iter()
                .filter(|s| s.ends_with(".sexp") || s.ends_with(".sexpr"))
                .cloned()
                .collect(),
            cpp: inputs
                .iter()
                .filter(|s| s.ends_with(".cpp") || s.ends_with(".cc"))
                .cloned()
                .collect(),
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
    print_logic_info(&d);
    if let Some(dump_gephi) = cli.dump_gephi {
        let cfg = GephiDumpCfg::from_prefix(&dump_gephi);
        dump_logic_graph_gephi(&d, &cfg).unwrap();
    }
    if let Some(output) = cli.output {
        let mut cpp = vec![];
        for file in input.cpp {
            cpp.push(absolute(&file).unwrap().to_string_lossy().to_string());
        }
        let extra = ExtraCfg {
            cpp,
            define: cli.defines,
        };
        cgen(&d, &cfg, &output, &extra).unwrap();
    }
}
