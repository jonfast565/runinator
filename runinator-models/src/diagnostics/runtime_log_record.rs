#[allow(unused_imports)]
use super::*;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RuntimeLogRecord {
    #[serde(default = "Uuid::now_v7")]
    pub event_id: Uuid,
    #[serde(default = "Utc::now")]
    pub occurred_at: DateTime<Utc>,
    pub source: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub runtime_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub replica_id: Option<Uuid>,
    pub level: String,
    pub target: String,
    pub message: String,
    #[serde(default, skip_serializing_if = "is_zero")]
    pub dropped_before: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub workflow_run_id: Option<Uuid>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub effect_id: Option<Uuid>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub trace_id: Option<Uuid>,
}

impl Validate for RuntimeLogRecord {
    fn validate(&self) -> Result<(), ValidationError> {
        required_text("source", &self.source, SHORT_TEXT_MAX)?;
        optional_text("runtime_id", self.runtime_id.as_deref(), SHORT_TEXT_MAX)?;
        required_text("level", &self.level, SHORT_TEXT_MAX)?;
        required_text("target", &self.target, SHORT_TEXT_MAX)?;
        required_text("message", &self.message, LONG_TEXT_MAX)
    }
}
