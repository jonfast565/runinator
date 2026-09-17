#[allow(unused_imports)]
use super::*;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkspaceDiff {
    pub before_revision: String,
    pub after_revision: String,
    pub changes: Vec<WorkspaceDifference>,
    pub next_cursor: Option<String>,
}
