use std::{fs::File, io::{BufReader, Read}, path::Path};

// use bincode::{Decode, Encode, config::LittleEndian};
use byteorder::{LittleEndian, ReadBytesExt};
use log::info;

// use crate::{BincodeExt, LegacyConfig};

pub type MHType = u32;
pub const MH_WIDTH: usize = 32;

pub type MinHash = [MHType; MH_WIDTH];

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct MHVer {
    pub dtype_bits: u32,
    pub width: u32,
}

pub const MH_VER: MHVer = MHVer {
    dtype_bits: MHType::BITS,
    width: MH_WIDTH as u32,
};

pub struct ThreadProf {
    pub act_cnt: Vec<u32>,
    pub act_hash: Vec<MinHash>,
    pub rdtsc_ticks: Vec<u64>,
}

#[repr(C)]
pub struct OrigBlockRecord {
    pub thread: u32,
    pub block: u32,
    pub orig_block: u32,
}

pub struct ProfData {
    pub threads: Vec<ThreadProf>,
    pub orig_blocks: Vec<OrigBlockRecord>,
}

impl ProfData {
    pub fn from_reader(reader: &mut impl Read) -> std::io::Result<Self> {
        let mh_ver_bits = reader.read_u32::<LittleEndian>()?;
        let mh_ver_width = reader.read_u32::<LittleEndian>()?;
        let mh_ver = MHVer {
            dtype_bits: mh_ver_bits,
            width: mh_ver_width,
        };
        assert_eq!(mh_ver, MH_VER);
        let part_count = reader.read_u64::<LittleEndian>()?;
        let mut threads = vec![];
        for _ in 0..part_count {
            let act_cnt_size = reader.read_u64::<LittleEndian>()? as usize;
            let act_cnt = unsafe {
                let mut vec = Vec::<u32>::with_capacity(act_cnt_size);
                vec.set_len(act_cnt_size);
                reader.read_u32_into::<LittleEndian>(&mut vec)?;
                vec
            };
            let act_hash_cnt = reader.read_u64::<LittleEndian>()? as usize;
            let act_hash = unsafe {
                let mut vec = Vec::<MinHash>::with_capacity(act_hash_cnt);
                vec.set_len(act_hash_cnt);
                let ptr = vec.as_mut_ptr() as *mut u32;
                let total_count = act_hash_cnt * MH_WIDTH;
                let buf = std::slice::from_raw_parts_mut(ptr, total_count);
                reader.read_u32_into::<LittleEndian>(buf)?;
                vec
            };
            let rdtsc_ticks_size = reader.read_u64::<LittleEndian>()? as usize;
            let rdtsc_ticks = unsafe {
                let mut vec = Vec::<u64>::with_capacity(rdtsc_ticks_size);
                vec.set_len(rdtsc_ticks_size);
                reader.read_u64_into::<LittleEndian>(&mut vec)?;
                vec
            };
            threads.push(ThreadProf {
                act_cnt,
                act_hash,
                rdtsc_ticks,
            });
        }
        let orig_blocks_size = reader.read_u64::<LittleEndian>()? as usize;
        let orig_blocks = unsafe {
            let mut vec = Vec::<OrigBlockRecord>::with_capacity(orig_blocks_size);
            vec.set_len(orig_blocks_size);
            let ptr = vec.as_mut_ptr() as *mut u32;
            let buf = std::slice::from_raw_parts_mut(ptr, orig_blocks_size * 3);
            reader.read_u32_into::<LittleEndian>(buf)?;
            vec
        };
        info!("Read {} threads, {} orig blocks", threads.len(), orig_blocks.len());
        Ok(Self { threads, orig_blocks })
    }
    pub fn from_file(path: impl AsRef<Path>) -> std::io::Result<Self> {
        let file = File::open(path.as_ref())?;
        let file_size = file.metadata()?.len();
        let mut file = BufReader::new(file);
        let start_time = std::time::Instant::now();
        let data = Self::from_reader(&mut file)?;
        let end_time = std::time::Instant::now();
        let duration = end_time - start_time;
        let size_mb = file_size as f64 / 1024.0 / 1024.0;
        let speed = size_mb / duration.as_millis() as f64 * 1000.0;
        info!("Read prof data from {} in {:?}, size {:.2} MB, speed: {:.2} MB/s", path.as_ref().display(), duration, size_mb, speed);
        Ok(data)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_from_file() {
        ProfData::from_file("build/boom_l_1.T2/prof/linux.bin").unwrap();
        assert!(false);
    }
}

// impl BincodeExt<LegacyConfig> for ProfData {}

// #[cfg(test)]
// mod tests {
//     use super::*;

//     // #[test]
//     // fn dump_mhash() {
//     //     let mh: MinHash = [1; _];
//     //     let data = ProfData {
//     //         mh_ver: MHVer {
//     //             dtype_bits: MHType::BITS,
//     //             width: MH_WIDTH as u32,
//     //         },
//     //         orig_blocks: vec![],
//     //         threads: vec![
//     //             ThreadProf {
//     //                 act_cnt: vec![],
//     //                 act_hash: vec![mh],
//     //                 rdtsc_ticks: vec![],
//     //             },
//     //         ],
//     //     };
//     //     data.write_file("tmp.bin").unwrap();
//     // }
//     #[test]
//     fn test_load_profdata() {
//         ProfData::read_file("build/boom_l_1.T2/prof/linux.bin").unwrap();
//     }
// }
