use crate::{orchestration::GateKind, value::Value};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AssertViolation {
    pub name: String,
    pub message: String,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AssertOutput {
    pub passed: bool,
    pub violations: Vec<AssertViolation>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TransformOutput {
    pub bindings: Value,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuditOutput {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub id: Option<Uuid>,
    pub actor: Option<String>,
    pub action: String,
    pub target: Option<String>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CheckpointOutput {
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub checkpoint_id: Option<Uuid>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MutexState {
    pub name: String,
    pub poll_interval: i64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub deadline_unix: Option<i64>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MutexOutput {
    pub name: String,
    pub acquired: bool,
    #[serde(default)]
    pub released: bool,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ThrottleState {
    pub name: String,
    pub poll_interval: i64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub deadline_unix: Option<i64>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ThrottleOutput {
    pub name: String,
    pub admitted: bool,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CooldownOutput {
    pub name: String,
    pub skipped: bool,
    pub remaining_seconds: i64,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AwaitWorkflowState {
    pub workflow_id: Uuid,
    pub workflow_name: String,
    /// When awaiting a RexRap task handle, join exactly this child run rather than every run of
    /// the workflow. Optional for backward-compatible persisted await state.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub exact_run_id: Option<Uuid>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub correlation_value: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub since_unix: Option<i64>,
    pub mode: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub deadline_unix: Option<i64>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AwaitWorkflowOutput {
    pub workflow_id: Uuid,
    pub matched_run_ids: Vec<Uuid>,
    pub mode: String,
    pub statuses: Vec<String>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DebounceState {
    pub deadline_unix: i64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub trigger_key: Option<String>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DebounceOutput {
    pub deadline_unix: i64,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CollectState {
    pub name: String,
    pub items: Vec<Value>,
    pub threshold: i64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub deadline_unix: Option<i64>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CollectOutput {
    pub items: Vec<Value>,
    pub count: usize,
    pub reason: String,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BarrierState {
    pub name: String,
    pub expected_count: i64,
    pub arrivals: Vec<Uuid>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub deadline_unix: Option<i64>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BarrierOutput {
    pub name: String,
    pub arrivals: Vec<Uuid>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CircuitBreakerState {
    pub name: String,
    pub circuit_state: String,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CircuitBreakerOutput {
    pub name: String,
    pub circuit_state: String,
    pub tripped: bool,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EventSourceState {
    pub event_type: String,
    pub events_processed: i64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub deadline_unix: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_events: Option<i64>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GateRecord {
    pub workflow_run_id: Uuid,
    pub node_id: String,
    pub kind: GateKind,
    pub status: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,
    #[serde(default)]
    pub condition: Value,
    pub metadata: Value,
}
