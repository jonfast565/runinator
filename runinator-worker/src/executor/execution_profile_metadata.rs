#[allow(unused_imports)]
use super::*;

pub(crate) struct ExecutionProfileMetadata {
    pub support: runinator_models::providers::ExecutionProfileSupport,
    pub credential_scopes: Vec<String>,
}
