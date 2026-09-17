#[allow(unused_imports)]
use super::*;

/// a stored setting's identity, without its value. returned by the list endpoint.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SettingSummary {
    pub id: Uuid,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub org_id: Option<Uuid>,
    pub scope: String,
    pub name: String,
    #[serde(default)]
    pub kind: SettingKind,
    /// expiry declared for a secret, if any. config entries never carry expiry metadata.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub expires_at: Option<DateTime<Utc>>,
}
