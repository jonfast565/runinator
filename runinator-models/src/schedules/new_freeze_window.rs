#[allow(unused_imports)]
use super::*;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NewFreezeWindow {
    #[serde(default)]
    pub org_id: Option<Uuid>,
    #[serde(default)]
    pub workflow_id: Option<Uuid>,
    pub name: String,
    #[serde(default)]
    pub reason: Option<String>,
    pub starts_at: DateTime<Utc>,
    pub ends_at: DateTime<Utc>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub schedule: Option<ScheduleSpec>,
    #[serde(default = "default_enabled")]
    pub enabled: bool,
}

impl Validate for NewFreezeWindow {
    fn validate(&self) -> Result<(), ValidationError> {
        required_text("name", &self.name, SHORT_TEXT_MAX)?;
        optional_text("reason", self.reason.as_deref(), LONG_TEXT_MAX)?;
        if self.ends_at <= self.starts_at {
            return Err(ValidationError::new(
                "ends_at",
                "must be later than starts_at",
            ));
        }
        if let Some(schedule) = &self.schedule {
            schedule.validate()?;
            if schedule.duration_seconds <= 0 {
                return Err(ValidationError::new(
                    "schedule.duration_seconds",
                    "a freeze window needs a positive duration",
                ));
            }
        }
        Ok(())
    }
}
