#[allow(unused_imports)]
use super::*;

#[derive(Debug, Deserialize)]
pub struct CredentialPutRequest {
    pub scope: String,
    pub name: String,
    pub value: Value,
    // declared json-schema, required once per config slot; ignored for secrets.
    #[serde(default)]
    pub schema: Option<Value>,
    #[serde(default)]
    pub kind: SettingKind,
    /// optional RFC 3339 expiry for secrets; rejected for config values.
    #[serde(default)]
    pub expires_at: Option<chrono::DateTime<chrono::Utc>>,
}

impl Validate for CredentialPutRequest {
    fn validate(&self) -> Result<(), ValidationError> {
        identifier("scope", &self.scope)?;
        identifier("name", &self.name)?;
        if self.kind == SettingKind::Config && self.expires_at.is_some() {
            return Err(ValidationError::new(
                "expires_at",
                "is only valid for secrets",
            ));
        }
        Ok(())
    }
}
