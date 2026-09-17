#[allow(unused_imports)]
use super::*;

/// marks a cursor as an interrupt handler rather than an ordinary thread of control.
///
/// every field defaults. a frame that silently degraded to `None` would un-suspend a cursor
/// mid-handler, but failing the parse is worse: `WorkflowExecutionState::from_state` falls back to
/// `unwrap_or_default`, which would discard every cursor in the run. so the frame is made
/// structurally incapable of failing to parse instead.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct InterruptFrame {
    /// the cursor this handler suspended, and will return control to.
    #[serde(default)]
    pub interrupted_cursor: Uuid,
    #[serde(default)]
    pub source: InterruptSource,
    /// what the raising event carried, readable in the region as `interrupt.payload`.
    #[serde(default, skip_serializing_if = "Value::is_null")]
    pub payload: Value,
    #[serde(default)]
    pub resume: ResumePoint,
    #[serde(default = "Utc::now")]
    pub raised_at: DateTime<Utc>,
}
