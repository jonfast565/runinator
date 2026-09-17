#[allow(unused_imports)]
use super::*;

#[derive(Debug, Clone, Deserialize)]
pub(super) struct EvalCase {
    pub(super) name: String,
    #[serde(default)]
    pub(super) workflow: Option<String>,
    #[serde(default)]
    pub(super) input: Value,
    #[serde(default)]
    pub(super) expect: Option<EvalExpectation>,
    #[serde(default)]
    pub(super) judge: Option<EvalJudge>,
}
