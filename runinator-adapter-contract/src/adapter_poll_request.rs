#[allow(unused_imports)]
use super::*;

/// A pull request made by the durable adapter scheduler. `checkpoint` is opaque to Runinator and
/// belongs to the adapter kind; implementations must return a replacement only after they have
/// completely enumerated the associated event batch.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AdapterPollRequest {
    #[serde(default)]
    pub configuration: Value,
    #[serde(default)]
    pub secrets: Value,
    #[serde(default)]
    pub checkpoint: Value,
    /// A first poll establishes a high-water mark without replaying history.
    #[serde(default)]
    pub initialize: bool,
}
