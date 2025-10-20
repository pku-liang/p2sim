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

#[derive(Debug, Clone, Serialize, Deserialize)]
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

impl State {
    pub fn is_array(&self) -> bool {
        !self.dims.is_empty()
    }
    pub fn element_count(&self) -> usize {
        self.dims.iter().product()
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

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "kebab-case")]
pub struct Block {
    pub always: bool,
    pub next: Vec<BlockId>,
    pub reads: Vec<StateId>,
    pub writes: Vec<StateId>,
    pub insts: Vec<Inst>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub struct Reset {
    pub reset: StateId,
    pub insts: Vec<Inst>,
    pub acts: Vec<BlockId>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub struct Design {
    pub name: String,
    pub max_concat: u32,
    pub ext_fns: Vec<String>,
    pub states: Vec<State>,
    pub blocks: Vec<Block>,
    pub resets: Vec<Reset>,
}
