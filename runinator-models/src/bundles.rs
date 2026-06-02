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

#[derive(Debug, Clone, Default, Serialize, serde::Deserialize)]
pub struct SecretBundle {
    #[serde(default)]
    pub secrets: Vec<SecretBundleEntry>,
}

#[derive(Debug, Clone, PartialEq, Serialize, serde::Deserialize)]
pub struct SecretBundleEntry {
    pub scope: String,
    pub name: String,
    // the typed payload: a JSON string for secrets, or arbitrary JSON for config.
    // a bare string still deserializes into `Value::String`, and `secret` is accepted
    // as an alias so back-compat bundles keep working.
    #[serde(alias = "secret")]
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
}

impl Bundle for SecretBundle {
    const RESOURCE: &'static str = "/credentials/import";
}
