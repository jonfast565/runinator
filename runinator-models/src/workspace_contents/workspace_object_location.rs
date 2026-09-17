#[allow(unused_imports)]
use super::*;

/// Server-validated physical location of one logical object within a workspace.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorkspaceObjectLocation {
    pub kind: u8,
    pub raw_len: u64,
    pub id: String,
    pub pack: String,
    pub offset: u64,
    pub length: u64,
    pub member: u32,
}
