#[allow(unused_imports)]
use super::*;

/// what a case asserts about the resulting simulation.
#[derive(Debug, Clone, Default, Deserialize)]
pub struct Expectations {
    /// the terminal run status.
    #[serde(default)]
    pub status: Option<WorkflowStatus>,
    /// nodes that must be visited.
    #[serde(default)]
    pub reached: Vec<String>,
    /// nodes that must not be visited.
    #[serde(default)]
    pub not_reached: Vec<String>,
    /// per-router-node expected next target: `{ "gate": "on" }`.
    #[serde(default)]
    pub branches: HashMap<String, String>,
    /// exact match on the run's final output.
    #[serde(default)]
    pub output: Option<Value>,
    /// subset match: every key/value here must be present in the final output object.
    #[serde(default)]
    pub output_contains: Option<Value>,
    /// whether the walk is expected to get stuck (an unsupported/blocked node with no edge).
    #[serde(default)]
    pub error: Option<bool>,
}
