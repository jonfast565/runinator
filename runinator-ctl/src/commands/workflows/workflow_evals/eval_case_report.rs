#[allow(unused_imports)]
use super::*;

#[derive(Debug, Clone, Serialize)]
pub(super) struct EvalCaseReport {
    pub(super) name: String,
    pub(super) workflow: String,
    pub(super) run_id: Uuid,
    pub(super) status: String,
    pub(super) passed: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(super) failure: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(super) judge_run_id: Option<Uuid>,
    pub(super) prompt_assets: Value,
}
