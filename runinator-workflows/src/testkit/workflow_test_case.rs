#[allow(unused_imports)]
use super::*;

/// one case: inputs, config fixtures, mocked node outcomes, and expectations to assert.
#[derive(Debug, Clone, Deserialize)]
pub struct WorkflowTestCase {
    pub name: String,
    /// the workflow this case targets, overriding the suite default.
    #[serde(default)]
    pub workflow: Option<String>,
    #[serde(default)]
    pub input: Value,
    /// the `config.*` tree exposed to expressions, shaped `{ scope: { name: value } }`.
    #[serde(default)]
    pub config: Value,
    /// mocked task/park outcomes keyed by node id.
    #[serde(default)]
    pub mocks: HashMap<String, MockSpec>,
    #[serde(default)]
    pub expect: Expectations,
}
