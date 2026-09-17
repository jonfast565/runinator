#[allow(unused_imports)]
use super::*;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SkippedOutput {
    pub skipped: bool,
    pub node_id: String,
}
