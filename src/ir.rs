use serde::{Deserialize, Serialize};

use index_vec_ext::index_ext;

index_ext! {
    #[derive(Serialize, Deserialize)]
    pub struct BlockId(pub u32) for Block, Reset
}

index_ext! {
    #[derive(Serialize, Deserialize)]
    pub struct StateId(pub u32) for State
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub struct Mirror {
    pub shared_id: StateId,
    pub ty: AccessType,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "kebab-case")]
pub struct State {
    pub name: String,
    pub width: usize,
    #[serde(default)]
    pub dims: Vec<usize>,
    #[serde(default)]
    pub is_ext: bool,
    #[serde(default)]
    pub is_local: bool,
    #[serde(default)]
    pub mirror: Option<Mirror>,
    #[serde(default)]
    pub is_disabled: bool,
}

pub struct StateLayout {
    pub align: usize,
    pub size: usize,
}

impl State {
    pub fn is_array(&self) -> bool {
        !self.dims.is_empty()
    }
    pub fn element_count(&self) -> usize {
        self.dims.iter().product()
    }
    pub fn get_align(&self) -> usize {
        match self.width {
            0 => 0,
            1..=8 => 1,
            9..=16 => 2,
            17..=32 => 4,
            _ => 8,
        }
    }
    pub fn get_element_size(&self) -> usize {
        match self.width {
            0 => 0,
            1..=8 => 1,
            9..=16 => 2,
            17..=32 => 4,
            33..=64 => 8,
            w => ((w + 7) / 8 + 7) / 8 * 8,
        }
    }
    pub fn get_size(&self) -> usize {
        self.get_element_size() * self.element_count()
    }
    pub fn get_layout(&self) -> StateLayout {
        let align = self.get_align();
        let size = self.get_size();
        StateLayout { align, size }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum AccessType {
    ReadOnly,
    ReadWrite,
    WriteOnly,
    BlackHole,
}

impl AccessType {
    pub fn from_rw(read: bool, write: bool) -> Self {
        match (read, write) {
            (true, true) => Self::ReadWrite,
            (true, false) => Self::ReadOnly,
            (false, true) => Self::WriteOnly,
            (false, false) => Self::BlackHole,
        }
    }
    pub fn has_read(self) -> bool {
        matches!(self, Self::ReadOnly | Self::ReadWrite)
    }
    pub fn has_write(self) -> bool {
        matches!(self, Self::WriteOnly | Self::ReadWrite)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Inst {
    /// Read state[sid] and assign to name
    /// ```cpp
    /// ty name = state[sid]
    /// ```
    Access {
        name: String,
        sid: StateId,
        ty: AccessType,
    },

    /// Prepare for copy on write on state[sid]
    /// ```cpp
    /// ty name = state[sid]
    /// ```
    /// alt.
    /// ```cpp
    /// ty & name = state[sid]
    /// ty name$old = state[sid]
    /// ```
    WritePre { name: String, sid: StateId },

    /// Execute C++ code
    /// ```cpp
    /// { code }
    /// ```
    CppCode {
        inst: String,
        #[serde(default)]
        upd: Option<StateId>,
    },

    /// Post write operation
    /// ```cpp
    /// if(name != state[sid]) state[sid] = name
    /// activate(diff, sid)
    /// ```
    /// alt.
    /// ```cpp
    /// bool diff = name != name$old;
    /// activate(diff, sid)
    /// ```
    WritePost {
        name: String,
        sid: StateId,
        acts: Vec<BlockId>,
    },
}

#[derive(Default, Clone, Serialize, Deserialize, Debug)]
pub struct StatePush {
    pub local_id: StateId,
    pub shared_id: StateId,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct PushTask {
    pub act_id: usize,
    pub pushes: Vec<StatePush>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "kebab-case")]
pub struct Block {
    #[serde(default)]
    pub orig_ids: Vec<BlockId>,
    pub always: bool,
    pub next: Vec<BlockId>,
    pub reads: Vec<StateId>,
    pub writes: Vec<StateId>,
    pub insts: Vec<Inst>,
    #[serde(default)]
    pub pushes: Option<PushTask>,
    #[serde(skip)]
    pub speculative: bool,
    #[serde(skip)]
    pub observer: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub struct Reset {
    pub reset: StateId,
    pub insts: Vec<Inst>,
    pub acts: Vec<BlockId>,
}

#[derive(Default, Clone, Debug, Serialize, Deserialize)]
pub struct StatePull {
    pub shared_id: StateId,
    pub local_id: StateId,
}

#[derive(Default, Clone, Debug, Serialize, Deserialize)]
pub struct PullTask {
    pub act_id: usize,
    pub pulls: Vec<StatePull>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub struct TPart {
    pub states: Vec<State>,
    #[serde(default)]
    pub pulls: Vec<PullTask>,
    pub blocks: Vec<Block>,
    pub resets: Vec<Reset>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub struct Design {
    pub name: String,
    pub max_concat: u32,
    pub ext_fns: Vec<String>,
    pub part: TPart,
}
