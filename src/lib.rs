#![feature(step_trait)]

pub mod act;
pub mod cgen;
pub mod comp;
pub mod ir;
pub mod mt;
pub mod pass;

use std::{
    fs::File,
    io::{BufWriter, Write},
    path::Path,
};

pub trait TemplateExt: askama::Template {
    fn write_into_file(&self, path: impl AsRef<Path>) -> std::io::Result<()> {
        let mut writer = BufWriter::new(File::create(path)?);
        askama::Template::write_into(self, &mut writer)?;
        writer.flush()?;
        Ok(())
    }
    fn write_into_file_with_values(
        &self,
        path: impl AsRef<Path>,
        options: &dyn askama::Values,
    ) -> std::io::Result<()> {
        let mut writer = BufWriter::new(File::create(path)?);
        askama::Template::write_into_with_values(self, &mut writer, options)?;
        writer.flush()?;
        Ok(())
    }
}
impl<T: askama::Template> TemplateExt for T {}

use bincode::{
    Decode, Encode,
    config::{Config, Configuration, Fixint, LittleEndian, NoLimit},
};

pub type LegacyConfig = Configuration<LittleEndian, Fixint, NoLimit>;

pub trait BincodeExt<C: Config + Default = Configuration>: Encode + Decode<()> {
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
