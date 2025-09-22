use std::{
    fs::File,
    io::{BufWriter, Write},
    path::Path,
};

use serde::{Deserialize, Serialize};
use serde_tuple::{Deserialize_tuple, Serialize_tuple};

use crate::index_ext;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum StateKind {
    RegSrc,
    RegDst,
    Input,
    Output,
    Others,
    Reader,
    Writer,
    Memory,
}

index_ext! {
    #[derive(Serialize, Deserialize)]
    pub struct LogicId(pub u32) for Logic
}

index_ext! {
    #[derive(Serialize, Deserialize)]
    pub struct StateId(pub u32) for State
}

#[derive(Debug, Clone, Serialize_tuple, Deserialize_tuple)]
pub struct State {
    pub kind: StateKind,
    pub name: String,
    pub width: usize,
    pub dims: Vec<usize>,
    pub acts: Vec<LogicId>,
}

impl State {
    pub fn is_array(&self) -> bool {
        !self.dims.is_empty()
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Inst {
    Raw(String),
    Save(String, usize),
    Act(Option<String>, Vec<LogicId>),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Logic {
    pub id: LogicId,
    pub always: bool,
    pub next: Vec<LogicId>,
    pub owned: Vec<StateId>,
    pub insts: Vec<Inst>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResetLogic {
    pub id: u32,
    pub reset: String,
    pub acts: Vec<LogicId>,
    pub insts: Vec<Inst>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub struct Design {
    pub name: String,
    pub max_concat: u32,
    pub states: Vec<State>,
    pub logics: Vec<Logic>,
    pub resets: Vec<ResetLogic>,
}

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
