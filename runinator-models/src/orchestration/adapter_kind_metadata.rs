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
    /// Human-readable provider setup steps. The command center renders these verbatim so dynamic
    /// adapter kinds can explain their installation without frontend-specific branching.
    #[serde(default)]
    pub setup_instructions: Vec<String>,
}
