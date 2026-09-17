#[allow(unused_imports)]
use super::*;

#[derive(Debug, Clone, Deserialize)]
pub struct CreatePersonalApiKeyRequest {
    pub name: String,
    #[serde(default)]
    pub org_id: Option<Uuid>,
    pub action_ceiling: Vec<Action>,
    #[serde(default)]
    pub expires_at: Option<DateTime<Utc>>,
}

impl Validate for CreatePersonalApiKeyRequest {
    fn validate(&self) -> Result<(), ValidationError> {
        required_text("name", &self.name, SHORT_TEXT_MAX)?;
        if self.action_ceiling.is_empty() {
            return Err(ValidationError::new(
                "action_ceiling",
                "must contain at least one action",
            ));
        }
        if self.action_ceiling.len() > 128 {
            return Err(ValidationError::new(
                "action_ceiling",
                "must contain at most 128 actions",
            ));
        }
        Ok(())
    }
}
