#[allow(unused_imports)]
use super::*;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorkspaceEntry {
    pub name: String,
    pub kind: String,
    pub inode_number: u64,
    pub content_id: String,
    pub size_bytes: u64,
    pub executable: bool,
    pub link_target: Option<String>,
}
