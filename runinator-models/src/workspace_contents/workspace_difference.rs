#[allow(unused_imports)]
use super::*;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkspaceDifference {
    pub path: String,
    pub result: bool,
    pub before: Option<String>,
    pub after: Option<String>,
}
