#[allow(unused_imports)]
use super::*;

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
