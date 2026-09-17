#[allow(unused_imports)]
use super::*;

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct NewNotificationPolicy {
    #[serde(default)]
    pub org_id: Option<Uuid>,
    #[serde(default)]
    pub workflow_id: Option<Uuid>,
    pub name: String,
    pub event: NotificationEvent,
    #[serde(default)]
    pub severity: NotificationSeverity,
    #[serde(default)]
    pub channel: NotificationChannel,
    #[serde(default)]
    pub provider: Option<String>,
    #[serde(default)]
    pub function: Option<String>,
    #[serde(default)]
    pub interactive: bool,
    #[serde(default)]
    pub target: Option<String>,
    #[serde(default)]
    pub threshold_seconds: Option<i64>,
    #[serde(default = "default_true")]
    pub enabled: bool,
    #[serde(default)]
    pub managed_by: Option<String>,
    #[serde(default)]
    pub configuration: Value,
}

impl Validate for NewNotificationPolicy {
    fn validate(&self) -> Result<(), ValidationError> {
        required_text("name", &self.name, SHORT_TEXT_MAX)?;
        optional_text("target", self.target.as_deref(), 2 * 1024)?;
        optional_text("managed_by", self.managed_by.as_deref(), SHORT_TEXT_MAX)?;
        optional_text("provider", self.provider.as_deref(), SHORT_TEXT_MAX)?;
        optional_text("function", self.function.as_deref(), SHORT_TEXT_MAX)?;
        if self.provider.is_some() != self.function.is_some() {
            return Err(ValidationError::new(
                "provider",
                "provider and function must be supplied together",
            ));
        }
        if let Some(provider) = &self.provider {
            required_text("provider", provider, SHORT_TEXT_MAX)?;
        }
        if let Some(function) = &self.function {
            required_text("function", function, SHORT_TEXT_MAX)?;
        }
        if let Some(seconds) = self.threshold_seconds
            && seconds <= 0
        {
            return Err(ValidationError::new(
                "threshold_seconds",
                "must be greater than zero",
            ));
        }
        if self.event.is_duration_based() && self.threshold_seconds.is_none() {
            return Err(ValidationError::new(
                "threshold_seconds",
                "is required for duration-based events",
            ));
        }
        Ok(())
    }
}
