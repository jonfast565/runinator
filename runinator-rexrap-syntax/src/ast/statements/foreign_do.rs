#[allow(unused_imports)]
use super::*;

/// a verbatim foreign-language compute block. lowers to `std.code` and runs on a worker.
#[derive(Debug, Clone, PartialEq)]
pub struct ForeignDo {
    pub language: String,
    pub source: String,
}
