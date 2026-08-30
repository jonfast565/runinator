use chrono::{DateTime, Utc};
use serde::{Serialize, de::DeserializeOwned};

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

#[derive(Debug, Clone, Default, Serialize, serde::Deserialize)]
pub struct SecretBundle {
    #[serde(default)]
    pub secrets: Vec<SecretBundleEntry>,
}

#[derive(Debug, Clone, PartialEq, Serialize, serde::Deserialize)]
pub struct SecretBundleEntry {
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

/// the result of importing a compiled pack zip at `/packs/import`: the imported workflow bundle,
/// the imported (redacted) secret bundle, and the pipelines that were upserted.
#[derive(Debug, Clone, Default, Serialize, serde::Deserialize)]
pub struct PackImportResult {
    #[serde(default)]
    pub workflows: crate::workflows::WorkflowBundle,
    #[serde(default)]
    pub secrets: SecretBundle,
    #[serde(default)]
    pub pipelines: Vec<crate::pipelines::Pipeline>,
}
