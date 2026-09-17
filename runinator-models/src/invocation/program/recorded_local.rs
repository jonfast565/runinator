#[allow(unused_imports)]
use super::*;

/// a `Local` call's observed value, kept so a resume or replay does not observe it again.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RecordedLocal {
    pub sequence: i64,
    pub name: String,
    pub value: Value,
}
