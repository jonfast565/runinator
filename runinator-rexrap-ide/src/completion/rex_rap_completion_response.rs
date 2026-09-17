#[allow(unused_imports)]
use super::*;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct RexRapCompletionResponse {
    pub replace_start_byte: usize,
    pub replace_end_byte: usize,
    pub items: Vec<RexRapCompletionItem>,
}
