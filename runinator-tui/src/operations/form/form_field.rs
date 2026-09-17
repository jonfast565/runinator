#[allow(unused_imports)]
use super::*;

#[derive(Debug)]
pub(super) struct FormField {
    pub(super) path: Vec<String>,
    pub(super) ty: RuninatorType,
    pub(super) required: bool,
    pub(super) default: Option<Value>,
}
