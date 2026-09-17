#[allow(unused_imports)]
use super::*;

#[derive(Debug, Clone, PartialEq)]
pub struct Reentry {
    pub max_visits: i64,
    pub on_exhausted: Option<Target>,
}
