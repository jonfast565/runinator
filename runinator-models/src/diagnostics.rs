//! Structured runtime diagnostics shared by producers, storage, APIs, and terminal clients.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::validation::{
    LONG_TEXT_MAX, SHORT_TEXT_MAX, Validate, ValidationError, optional_text, required_text,
};

pub const MAX_RUNTIME_LOG_BATCH: usize = 256;
pub const MAX_RUNTIME_LOG_BATCH_BYTES: usize = 64 * 1024;

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

fn is_zero(value: &u64) -> bool {
    *value == 0
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RuntimeLogBatch {
    pub records: Vec<RuntimeLogRecord>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct RuntimeLogQuery {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub workflow_run_id: Option<Uuid>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub effect_id: Option<Uuid>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub level: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub text: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub from: Option<DateTime<Utc>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub until: Option<DateTime<Utc>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cursor: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub limit: Option<i64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RuntimeLogPage {
    pub records: Vec<RuntimeLogRecord>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub next_cursor: Option<String>,
    pub dropped: u64,
    pub retention_seconds: i64,
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

impl Validate for RuntimeLogBatch {
    fn validate(&self) -> Result<(), ValidationError> {
        if self.records.len() > MAX_RUNTIME_LOG_BATCH {
            return Err(ValidationError::new(
                "records",
                format!("must contain at most {MAX_RUNTIME_LOG_BATCH} records"),
            ));
        }
        let mut bytes = 0usize;
        for (index, record) in self.records.iter().enumerate() {
            record.validate().map_err(|error| {
                ValidationError::new(format!("records.{index}.{}", error.path), error.message)
            })?;
            bytes = bytes.saturating_add(record.message.len());
        }
        if bytes > MAX_RUNTIME_LOG_BATCH_BYTES {
            return Err(ValidationError::new(
                "records",
                format!("messages must total at most {MAX_RUNTIME_LOG_BATCH_BYTES} bytes"),
            ));
        }
        Ok(())
    }
}
