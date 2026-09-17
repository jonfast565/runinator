#[allow(unused_imports)]
use super::*;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AssertOutput {
    pub passed: bool,
    pub violations: Vec<AssertViolation>,
}
