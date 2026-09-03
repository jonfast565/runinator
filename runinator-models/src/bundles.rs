use chrono::{DateTime, Utc};
use serde::{Serialize, de::DeserializeOwned};

use crate::execution_profiles::{ExecutionProfile, ExecutionProfilePutRequest};
use crate::providers::ProviderMetadata;
use crate::settings::SettingKind;
use crate::value::Value;

/// Marker trait for typed import bundles posted to the web service.
///
/// Implementations advertise their HTTP resource path so the API client and
/// importer can be generic over bundle kind.
pub trait Bundle: Serialize + DeserializeOwned + Send + Sync + 'static {
    const RESOURCE: &'static str;
}

#[derive(Debug, Clone, Default, Serialize, serde::Deserialize)]
pub struct ProviderBundle {
    #[serde(default)]
    pub providers: Vec<ProviderMetadata>,
}

impl Bundle for ProviderBundle {
    const RESOURCE: &'static str = "/providers/import";
}

impl crate::validation::Validate for ProviderBundle {
    fn validate(&self) -> Result<(), crate::validation::ValidationError> {
        for (index, provider) in self.providers.iter().enumerate() {
            crate::validation::Validate::validate(provider).map_err(|error| {
                crate::validation::ValidationError::new(
                    format!("providers[{index}].{}", error.path),
                    error.message,
                )
            })?;
        }
        crate::validation::serialized("provider_bundle", self)
    }
}

const fn default_settings_bundle_version() -> u32 {
    1
}

#[derive(Debug, Clone, PartialEq, Serialize, serde::Deserialize)]
pub struct SettingBundleEntry {
    pub scope: String,
    pub name: String,
    // The typed payload: a JSON string for secrets, or arbitrary JSON for config.
    pub value: Value,
    // optional declared json-schema for a config value; when omitted the web service infers one
    // from the first value and pins it per (scope, name). secrets are implicitly string-typed.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub schema: Option<Value>,
    // distinguishes a redacted secret from a non-sensitive config value; defaults to
    // secret so existing bundles import unchanged.
    #[serde(default)]
    pub kind: SettingKind,
    // modification time used to reconcile imports: an existing entry is only
    // overwritten when an incoming entry is strictly newer.
    #[serde(default)]
    pub updated_at: Option<DateTime<Utc>>,
    // optional secret expiry used by the engine's ahead-of-expiry notification scan. config entries
    // reject this metadata.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub expires_at: Option<DateTime<Utc>>,
}

#[derive(Debug, Clone, PartialEq, Serialize, serde::Deserialize)]
pub struct ExecutionProfileBundleEntry {
    pub configuration: ExecutionProfilePutRequest,
    #[serde(default)]
    pub updated_at: Option<DateTime<Utc>>,
}

/// Safe result metadata for a profile definition reconciled during pack import. Publication
/// revisions, publisher identities, archive digests, and archive contents are intentionally absent.
#[derive(Debug, Clone, PartialEq, Serialize, serde::Deserialize)]
pub struct ExecutionProfileImportResult {
    pub id: uuid::Uuid,
    pub org_id: Option<uuid::Uuid>,
    pub configuration: ExecutionProfilePutRequest,
    pub config_version: i64,
    pub updated_at: DateTime<Utc>,
}

impl From<ExecutionProfile> for ExecutionProfileImportResult {
    fn from(profile: ExecutionProfile) -> Self {
        Self {
            id: profile.id,
            org_id: profile.org_id,
            configuration: ExecutionProfilePutRequest {
                name: profile.name,
                description: profile.description,
                credential_scopes: profile.credential_scopes,
                collection: profile.collection,
                exposure: profile.exposure,
                enabled: profile.enabled,
            },
            config_version: profile.config_version,
            updated_at: profile.updated_at,
        }
    }
}

#[derive(Debug, Clone, Serialize, serde::Deserialize)]
pub struct SettingsBundle {
    #[serde(default = "default_settings_bundle_version")]
    pub version: u32,
    #[serde(default, alias = "secrets")]
    pub settings: Vec<SettingBundleEntry>,
    #[serde(default)]
    pub execution_profiles: Vec<ExecutionProfileBundleEntry>,
}

impl Default for SettingsBundle {
    fn default() -> Self {
        Self {
            version: default_settings_bundle_version(),
            settings: Vec::new(),
            execution_profiles: Vec::new(),
        }
    }
}

/// Compatibility names retained for downstream callers during the settings-wire transition.
pub type SecretBundle = SettingsBundle;
pub type SecretBundleEntry = SettingBundleEntry;

/// the result of importing a compiled pack zip at `/packs/import`: the imported workflow bundle,
/// the imported (redacted) secret bundle, and the pipelines that were upserted.
#[derive(Debug, Clone, Default, Serialize, serde::Deserialize)]
pub struct PackImportResult {
    #[serde(default)]
    pub workflows: crate::workflows::WorkflowBundle,
    #[serde(default)]
    #[serde(alias = "secrets")]
    pub settings: SettingsBundle,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub execution_profiles: Vec<ExecutionProfileImportResult>,
    #[serde(default)]
    pub pipelines: Vec<crate::pipelines::Pipeline>,
}
