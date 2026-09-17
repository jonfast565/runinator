#[allow(unused_imports)]
use super::*;

#[derive(Default, Deserialize)]
pub struct WorkflowPublishOptions {
    pub contract_override_reason: Option<String>,
}
