#[allow(unused_imports)]
use super::*;

pub(super) struct CodeRequest {
    pub(super) language: String,
    pub(super) source: String,
    pub(super) runtime: CodeRuntime,
    pub(super) context: Value,
    pub(super) expected_output_type: Option<RuninatorType>,
}
