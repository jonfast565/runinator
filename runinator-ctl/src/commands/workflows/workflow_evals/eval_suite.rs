#[allow(unused_imports)]
use super::*;

#[derive(Debug, Clone, Deserialize)]
pub(super) struct EvalSuite {
    #[serde(default)]
    pub(super) workflow: Option<String>,
    pub(super) cases: Vec<EvalCase>,
}
