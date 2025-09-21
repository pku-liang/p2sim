use std::fs::File;

use clap::Parser;
use ggsim::{
    cgen::{CGenCfg, cgen},
    compile::parse,
    ir::Design,
};
use memmap2::Mmap;

#[derive(Parser)]
struct Cli {
    input: String,
    #[arg(short, long)]
    output: String,
}

fn parse_file(input: &str) -> Design {
    let file = File::open(input).unwrap();
    let mmap = unsafe { Mmap::map(&file).unwrap() };
    let data = unsafe { str::from_utf8_unchecked(&mmap[..]) };
    parse(data).unwrap()
}

fn main() {
    let cli = Cli::parse();
    let d = parse_file(&cli.input);
    let cfg = CGenCfg {
        act_pack: 32,
        split_size: 8192,
    };
    cgen(&d, &cfg, &cli.output).unwrap();
}
