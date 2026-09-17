#[allow(unused_imports)]
use super::*;

#[derive(Debug, Clone, Deserialize)]
pub(super) struct EvalJudge {
    pub(super) workflow: String,
    #[serde(default)]
    pub(super) input: Value,
    #[serde(default = "default_judge_pointer")]
    pub(super) pass_pointer: String,
}
