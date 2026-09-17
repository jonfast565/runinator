#[allow(unused_imports)]
use super::*;

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Completion {
    pub start: usize,
    pub options: Vec<String>,
    pub hint: Option<String>,
}
