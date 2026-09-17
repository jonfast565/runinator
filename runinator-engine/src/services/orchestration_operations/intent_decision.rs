#[allow(unused_imports)]
use super::*;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IntentDecision {
    pub matched: Vec<String>,
    pub winner: Option<String>,
    pub suppressed: Vec<String>,
}
