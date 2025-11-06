use bincode::{Decode, Encode};

use crate::{BincodeExt, LegacyConfig};

pub type MHType = u32;
pub const MH_WIDTH: usize = 32;

pub type MinHash = [MHType; MH_WIDTH];

#[derive(Clone, Copy, PartialEq, Eq, Encode, Decode)]
pub struct MHVer {
    pub dtype_bits: u32,
    pub width: u32,
}

pub const MH_VER: MHVer = MHVer {
    dtype_bits: MHType::BITS,
    width: MH_WIDTH as u32,
};

#[derive(Encode, Decode)]
pub struct ThreadProf {
    pub act_cnt: Vec<u32>,
    pub act_hash: Vec<MinHash>,
    pub rdtsc_ticks: Vec<u64>,
}

#[derive(Encode, Decode)]
pub struct OrigBlockRecord {
    pub thread: u32,
    pub block: u32,
    pub orig_block: u32,
}

#[derive(Encode, Decode)]
pub struct ProfData {
    pub mh_ver: MHVer,
    pub orig_blocks: Vec<OrigBlockRecord>,
    pub threads: Vec<ThreadProf>,
}

impl BincodeExt<LegacyConfig> for ProfData {}


#[cfg(test)]
mod tests {
    use super::*;

    // #[test]
    // fn dump_mhash() {
    //     let mh: MinHash = [1; _];
    //     let data = ProfData {
    //         mh_ver: MHVer {
    //             dtype_bits: MHType::BITS,
    //             width: MH_WIDTH as u32,
    //         },
    //         orig_blocks: vec![],
    //         threads: vec![
    //             ThreadProf {
    //                 act_cnt: vec![],
    //                 act_hash: vec![mh],
    //                 rdtsc_ticks: vec![],
    //             },
    //         ],
    //     };
    //     data.write_file("tmp.bin").unwrap();
    // }
    #[test]
    fn test_load_profdata() {
        ProfData::read_file("pgo-data/boom_m_1.T1.prof.bin").unwrap();
    }
}