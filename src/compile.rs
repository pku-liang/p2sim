use crate::ir::*;

pub fn parse(s: &str) -> serde_lexpr::Result<Design> {
    serde_lexpr::from_str(s)
}
