#[allow(unused_imports)]
use super::*;

/// a directed link between two member workflows (by canonical path), realized as a `chained` trigger on the
/// `from` workflow targeting the `to` workflow.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PipelineLinkSpec {
    pub from: String,
    pub to: String,
    #[serde(default)]
    pub on: PipelineLinkSelector,
    #[serde(default = "default_true")]
    pub enabled: bool,
    /// pure expression object over `params`, `source`, and `members`, overlaid onto pipeline input.
    #[serde(default)]
    pub parameters: Value,
}
