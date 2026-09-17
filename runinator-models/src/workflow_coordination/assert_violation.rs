#[allow(unused_imports)]
use super::*;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AssertViolation {
    pub name: String,
    pub message: String,
}
