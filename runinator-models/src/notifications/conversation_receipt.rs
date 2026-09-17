#[allow(unused_imports)]
use super::*;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ConversationReceipt {
    pub source: String,
    pub scope: String,
    pub correlation_key: String,
}
