#[allow(unused_imports)]
use super::*;

#[derive(Debug, Clone)]
pub(crate) struct ResourceImport {
    pub kind: String,
    pub path: String,
    pub alias: String,
    pub revision: Option<i64>,
}
