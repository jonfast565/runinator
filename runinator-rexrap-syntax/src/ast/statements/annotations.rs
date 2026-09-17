#[allow(unused_imports)]
use super::*;

#[derive(Debug, Clone, Default, PartialEq)]
pub struct Annotations {
    pub id: Option<String>,
    pub skip: bool,
    pub locked: bool,
    pub timeout_seconds: Option<i64>,
}
