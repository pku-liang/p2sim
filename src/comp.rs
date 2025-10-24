use std::{fs::File, io::BufReader, path::Path};

use crate::ir::*;

pub fn parse(s: &str) -> serde_lexpr::Result<TPart> {
    serde_lexpr::from_str(s)
}

pub fn parse_file(path: impl AsRef<Path>) -> std::io::Result<Design> {
    let file = File::open(path)?;
    let reader = BufReader::new(file);
    let result = serde_lexpr::from_reader(reader)?;
    Ok(result)
}
