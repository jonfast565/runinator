#[allow(unused_imports)]
use super::*;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
pub struct ResultMapping {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub subject_revision: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub resources: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub evidence: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub failure_class: Option<String>,
    /// JSON pointer to an array of `{source, scope, correlation_key}` identities that should route
    /// future ingress to this binding generation.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub correlations: Option<String>,
    /// JSON pointer to an object merged into the binding resources after a phase succeeds. This
    /// carries compact state such as a reviewed plan or a report outline without replacing
    /// unrelated mission context.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub resources_patch: Option<String>,
    /// JSON pointer to a string naming the next declared phase. The reducer starts it as a new
    /// immutable epoch; it never rewires the current pipeline graph in place.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub next_member: Option<String>,
}
