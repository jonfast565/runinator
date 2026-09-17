#[allow(unused_imports)]
use super::*;

pub(crate) struct FnEntry {
    pub(crate) name: String,
    pub(crate) signature: String,
    pub(crate) recursive: Option<i64>,
    pub(crate) body: FnBodyForm,
}
