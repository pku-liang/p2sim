#![feature(step_trait)]

pub mod cgen;
pub mod comp;
pub mod ir;
pub mod pass;
// pub mod cpp;

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
