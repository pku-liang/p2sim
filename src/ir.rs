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
    ExtIn,
    ExtOut,
    Readwriter,
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
    pub is_seq: bool,
    pub uses: Vec<LogicId>,
    pub modifies: Vec<LogicId>,
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

pub fn print_design_info(design: &Design) {
    eprintln!("Design: {}", design.name);
    eprintln!("  States: {}", design.states.len());
    let num_seq = design.states.iter().filter(|s| s.is_seq).count();
    eprintln!(
        "    Seq: {} ({:.2}%)",
        num_seq,
        num_seq as f64 / design.states.len() as f64 * 100.0
    );
    let num_mem = design
        .states
        .iter()
        .filter(|s| s.kind == StateKind::Memory)
        .count();
    eprintln!(
        "    Mem: {} ({:.2}%)",
        num_mem,
        num_mem as f64 / design.states.len() as f64 * 100.0
    );
    eprintln!("  Logics: {}", design.logics.len());
    let total_next_num = design.logics.iter().map(|l| l.next.len()).sum::<usize>();
    eprintln!(
        "    Avg Next: {:.2}",
        total_next_num as f64 / design.logics.len() as f64
    );
    let total_own_num = design.logics.iter().map(|l| l.own.len()).sum::<usize>();
    eprintln!(
        "    Avg Own: {:.2}",
        total_own_num as f64 / design.logics.len() as f64
    );
    let total_inst_num = design.logics.iter().map(|l| l.insts.len()).sum::<usize>();
    eprintln!(
        "    Avg Inst: {:.2}",
        total_inst_num as f64 / design.logics.len() as f64
    );
}
