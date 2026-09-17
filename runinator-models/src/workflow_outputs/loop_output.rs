#[allow(unused_imports)]
use super::*;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LoopOutput {
    pub index: i64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub item: Option<Value>,
    pub has_next: bool,
    pub count: usize,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last: Option<Value>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub results: Vec<Value>,
}
