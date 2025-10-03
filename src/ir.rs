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
    Reset,
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
    pub writes: Vec<LogicId>,
    pub creads: Vec<LogicId>,
    pub sreads: Vec<LogicId>,
    pub acts: Vec<LogicId>,
}

impl State {
    pub fn is_array(&self) -> bool {
        !self.dims.is_empty()
    }
    pub fn element_count(&self) -> usize {
        self.dims.iter().product()
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Inst {
    Raw(String),
    Save(StateId, String, bool, usize),
    Act(StateId, String, bool, Vec<LogicId>),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Logic {
    pub id: LogicId,
    pub always: bool,
    pub own: Vec<StateId>,
    pub next: Vec<LogicId>,
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
