#[allow(unused_imports)]
use super::*;

/// A durable, deduplicated piece of output produced while an effect is executing. Output events
/// are addressed by effect/continuation identity and never by a graph node-run.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct WorkflowEffectOutputEvent {
    pub event_id: Uuid,
    pub effect_id: Uuid,
    pub workflow_run_id: Uuid,
    pub continuation_id: Uuid,
    pub attempt: u32,
    /// Derived from the output kind by the backend rather than persisted independently.
    #[serde(default)]
    pub timeline_category: WorkflowTimelineCategory,
    pub output: WorkflowEffectOutput,
    pub created_at: i64,
}
