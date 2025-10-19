use std::{fs::File, io::Read, path::Path};

use bincode::Decode;

#[derive(Debug, Clone, Decode)]
pub struct ActInfo {
    pub act_cnt: Vec<u32>,
    pub act_hash: Vec<[u16; Self::HASH_WIDTH]>,
}

impl ActInfo {
    pub const HASH_WIDTH: usize = 32;
    pub fn from_reader(reader: &mut impl Read) -> Result<Self, bincode::error::DecodeError> {
        bincode::decode_from_std_read(reader, bincode::config::legacy())
    }
    pub fn from_file(path: impl AsRef<Path>) -> std::io::Result<Self> {
        let mut file = File::open(path)?;
        let res = Self::from_reader(&mut file)
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e))?;
        Ok(res)
    }
    pub fn query_similarity_nodiv(&self, a1: usize, a2: usize) -> i32 {
        let hash1 = &self.act_hash[a1];
        let hash2 = &self.act_hash[a2];
        let mut inters = 0;
        for i in 0..Self::HASH_WIDTH {
            if hash1[i] == hash2[i] {
                inters += 1;
            }
        }
        inters
    }
    pub fn query_similarity(&self, a1: usize, a2: usize) -> f64 {
        self.query_similarity_nodiv(a1, a2) as f64 / Self::HASH_WIDTH as f64
    }
}
