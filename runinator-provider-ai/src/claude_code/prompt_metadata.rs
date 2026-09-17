#[allow(unused_imports)]
use super::*;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct PromptMetadata {
    pub(super) asset: Option<String>,
    pub(super) digest: String,
    pub(super) source: &'static str,
}
