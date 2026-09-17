#[allow(unused_imports)]
use super::*;

#[derive(Debug, Clone, Deserialize)]
pub(super) struct EvalExpectation {
    #[serde(default)]
    pub(super) pointer: String,
    pub(super) equals: Value,
}
