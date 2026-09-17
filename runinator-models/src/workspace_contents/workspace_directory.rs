#[allow(unused_imports)]
use super::*;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorkspaceDirectory {
    pub revision_id: String,
    pub path: String,
    pub entries: Vec<WorkspaceEntry>,
    pub next_cursor: Option<String>,
}
