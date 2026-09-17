#[allow(unused_imports)]
use super::*;

#[derive(Debug, Clone, Serialize)]
pub(super) struct EvalReport {
    pub(super) eval_id: Uuid,
    pub(super) pack: String,
    pub(super) total: usize,
    pub(super) passed: usize,
    pub(super) failed: usize,
    pub(super) agreement_rate: f64,
    pub(super) cases: Vec<EvalCaseReport>,
}
