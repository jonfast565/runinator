#[allow(unused_imports)]
use super::*;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct DatabaseTablePolicy {
    pub table: &'static str,
    pub policy: TableDataPolicy,
}
