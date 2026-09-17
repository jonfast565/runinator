#[allow(unused_imports)]
use super::*;

#[derive(Debug, Clone, Deserialize)]
pub struct CreateApiKeyRequest {
    pub name: String,
    pub principal_kind: PrincipalKind,
    pub principal_id: Uuid,
    #[serde(default)]
    pub system_role: Option<SystemRole>,
    #[serde(default)]
    pub org_id: Option<Uuid>,
    #[serde(default)]
    pub action_ceiling: Vec<Action>,
    #[serde(default)]
    pub expires_at: Option<DateTime<Utc>>,
}

impl Validate for CreateApiKeyRequest {
    fn validate(&self) -> Result<(), ValidationError> {
        required_text("name", &self.name, SHORT_TEXT_MAX)?;
        if self.action_ceiling.len() > 128 {
            return Err(ValidationError::new(
                "action_ceiling",
                "must contain at most 128 actions",
            ));
        }
        Ok(())
    }
}
