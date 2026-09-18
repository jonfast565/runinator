#[allow(unused_imports)]
use super::*;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AdapterKindMetadata {
    pub kind: String,
    pub version: String,
    pub display_name: String,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub fields: Vec<AdapterConfigurationField>,
    /// Configuration fields shown when the polling transport is selected.
    #[serde(default)]
    pub polling_fields: Vec<AdapterConfigurationField>,
    #[serde(default)]
    pub event_names: Vec<String>,
    #[serde(default)]
    pub canonical_pointers: Vec<String>,
    #[serde(default)]
    pub capabilities: Vec<String>,
    /// Authentication modes accepted when this kind is configured for durable polling.
    #[serde(default)]
    pub polling_authentication: Vec<AdapterAuthenticationKind>,
    /// Secret bindings accepted when polling uses stored API credentials.
    #[serde(default)]
    pub polling_secret_fields: Vec<AdapterConfigurationField>,
    /// Credential scopes required from a selected execution profile.
    #[serde(default)]
    pub execution_profile_scopes: Vec<String>,
    /// Labels applied when an execution profile is selected for polling.
    #[serde(default)]
    pub execution_profile_required_labels: BTreeMap<String, String>,
    /// Configuration keys whose values form the adapter's durable identity.
    #[serde(default)]
    pub identity_fields: Vec<String>,
    /// The shape of the `scope` this kind stamps on the events it emits, with `{placeholder}`
    /// standing for a value taken from the adapter's configuration or its payload — GitHub emits
    /// `github:repository:{repository_id}`, Jira emits `{routing_scope}`.
    ///
    /// It exists so an ingress scope can be checked against what any installed kind could actually
    /// produce. An unreachable scope is otherwise accepted, stored, hashed into the revision
    /// digest, and silently never matched: the workflow simply never starts, with nothing anywhere
    /// saying why.
    #[serde(default)]
    pub scope_template: Option<String>,
    /// Human-readable provider setup steps. The command center renders these verbatim so dynamic
    /// adapter kinds can explain their installation without frontend-specific branching.
    #[serde(default)]
    pub setup_instructions: Vec<String>,
}
