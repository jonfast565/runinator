#[allow(unused_imports)]
use super::*;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DebounceOutput {
    pub deadline_unix: i64,
}
