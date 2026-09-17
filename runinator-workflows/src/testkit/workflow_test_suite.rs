#[allow(unused_imports)]
use super::*;

/// a `.rexrapt` test suite: a set of cases run against one compiled workflow (or, for multi-workflow
/// packs, the workflow each case names).
#[derive(Debug, Clone, Deserialize)]
pub struct WorkflowTestSuite {
    /// default workflow name for cases that do not name their own; optional for single-workflow packs.
    #[serde(default)]
    pub workflow: Option<String>,
    pub tests: Vec<WorkflowTestCase>,
}
