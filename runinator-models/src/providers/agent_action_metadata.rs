#[allow(unused_imports)]
use super::*;

/// Catalog-declared semantics used by mission authoring without coupling clients to a provider.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct AgentActionMetadata {
    pub prompt_parameter: String,
    pub response_text_pointer: String,
}
