#[allow(unused_imports)]
use super::*;

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ArgumentSpec {
    pub label: String,
    pub help: String,
}
