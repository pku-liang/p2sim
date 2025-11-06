use std::{fs::File, io::Read, path::Path};

use bincode::{
    Decode, Encode,
    config::{Config, Configuration, Fixint, LittleEndian, NoLimit},
};
use log::info;
use rustc_hash::FxHashMap;

use crate::ir::BlockId;

pub trait BincodeExt<C: Config + Default>: Encode + Decode<()> {
    fn write_file(&self, path: impl AsRef<Path>) -> std::io::Result<()> {
        let mut file = File::create(path)?;
        bincode::encode_into_std_write(self, &mut file, C::default())
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e))?;
        Ok(())
    }
    fn read_file(path: impl AsRef<Path>) -> std::io::Result<Self> {
        let mut file = File::open(path)?;
        bincode::decode_from_std_read(&mut file, C::default())
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e))
    }
}

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

#[derive(Encode, Decode)]
pub struct BlockIdMap {
    // [part, block_id] => [orig_block_id]
    pub map: Vec<Vec<Vec<u32>>>,
}
impl BincodeExt<Configuration> for BlockIdMap {}

#[derive(Decode, Encode)]
pub struct ProfData {
    pub th_prof_cnt: Vec<Vec<u64>>,
    pub th_blk_act_cnt: Vec<Vec<u32>>,
}
impl BincodeExt<Configuration<LittleEndian, Fixint, NoLimit>> for ProfData {}

pub fn get_block_act(id_map: &BlockIdMap, prof: &ProfData) -> FxHashMap<BlockId, u32> {
    info!("build block act map");
    assert_eq!(id_map.map.len(), prof.th_blk_act_cnt.len());
    let mut res = FxHashMap::default();
    for (th_id_map, th_blk_act) in id_map.map.iter().zip(prof.th_blk_act_cnt.iter()) {
        for (act, orig_ids) in th_blk_act.iter().zip(th_id_map.iter()) {
            for orig_id in orig_ids {
                res.insert(BlockId(*orig_id), *act);
            }
        }
    }
    info!("total blocks: {}", res.len());
    res
}

#[cfg(test)]
mod test {
    use super::*;

    #[test]
    fn example_dump() {
        let a = ProfData {
            th_prof_cnt: vec![vec![1], vec![2, 3]],
            th_blk_act_cnt: vec![vec![4], vec![5, 6]],
        };
        a.write_file("tmp.bin").unwrap();
    }
}
