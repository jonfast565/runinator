use crate::{value::Value, workflows::WorkflowStatus};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// one edge walked by a workflow run, derived from the node-run chain (`prev_node_run_id`).
/// `from_node` is `None` for the run's first node.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NodeTransition {
    pub from_node: Option<String>,
    pub to_node: String,
    pub reason: Option<String>,
    pub node_run_id: Uuid,
    pub at: DateTime<Utc>,
}

/// an aggregated `from_node -> to_node` edge across all runs of a workflow, with how often it
/// was taken and when it was last taken.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NodeTransitionStat {
    pub from_node: String,
    pub to_node: String,
    pub count: i64,
    pub last_reason: Option<String>,
    pub last_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExternalItem {
    pub id: Option<Uuid>,
    pub provider: String,
    pub resource_type: String,
    pub external_id: String,
    pub status: String,
    #[serde(default)]
    pub title: Option<String>,
    #[serde(default)]
    pub url: Option<String>,
    #[serde(default)]
    pub metadata: Value,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ApprovalRequest {
    pub id: Option<Uuid>,
    pub workflow_run_id: Uuid,
    pub node_id: String,
    pub approval_type: String,
    pub status: String,
    pub prompt: String,
    #[serde(default)]
    pub resolved_by: Option<String>,
    #[serde(default)]
    pub resolved_at: Option<DateTime<Utc>>,
    #[serde(default)]
    pub metadata: Value,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

/// how a gate is resolved: `manual` (opened/closed from the ui), `condition` (the reducer
/// auto-evaluates a wdl boolean), or `external` (status set via the api by an outside system).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GateKind {
    Manual,
    Condition,
    External,
}

/// a per-run, per-node gate: a workflow blocks on it until its status reaches `open`/`passed`.
/// distinct from an `ApprovalRequest` (a human decision) — a gate is an automated/policy check.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Gate {
    pub id: Option<Uuid>,
    pub workflow_run_id: Uuid,
    pub node_id: String,
    pub kind: GateKind,
    pub status: String,
    #[serde(default)]
    pub label: Option<String>,
    #[serde(default)]
    pub condition: Value,
    #[serde(default)]
    pub reason: Option<String>,
    #[serde(default)]
    pub resolved_by: Option<String>,
    #[serde(default)]
    pub resolved_at: Option<DateTime<Utc>>,
    #[serde(default)]
    pub metadata: Value,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AutomationEvent {
    pub id: Option<Uuid>,
    #[serde(default)]
    pub workflow_run_id: Option<Uuid>,
    #[serde(default)]
    pub external_item_id: Option<Uuid>,
    pub provider: String,
    pub event_type: String,
    pub message: String,
    #[serde(default)]
    pub metadata: Value,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CatalogItem {
    pub id: Option<Uuid>,
    pub uri: String,
    pub item_type: String,
    pub name: String,
    pub version: String,
    #[serde(default)]
    pub document: Value,
    #[serde(default)]
    pub metadata: Value,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IdempotencyKey {
    pub id: Option<Uuid>,
    pub scope: String,
    pub key: String,
    #[serde(default)]
    pub result: Value,
    pub created_at: DateTime<Utc>,
}

/// scope every action-node idempotency key is stored under, keeping the reserved keys the platform
/// manages separate from the caller-chosen scopes of the manual put/get store. the workflow
/// qualification lives inside the key itself, stamped by the reducer.
pub const ACTION_IDEMPOTENCY_SCOPE: &str = "action";

/// outcome of reserving an idempotency key for an action node, decided in one statement so
/// concurrent claimants cannot both acquire.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "outcome", rename_all = "snake_case")]
pub enum IdempotencyClaim {
    /// the caller now owns the key and must execute, then record the outcome against it.
    Acquired,
    /// an execution already completed under this key; replay `result` instead of executing.
    Completed { result: Value },
    /// a different node run holds an unfinished reservation, so this delivery is a concurrent
    /// duplicate.
    Held { owner_node_run_id: Uuid },
}

/// request body for reserving an action node's idempotency key.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IdempotencyClaimRequest {
    pub scope: String,
    pub key: String,
    pub owner_node_run_id: Uuid,
    /// the claimant's own execution deadline in seconds; a reservation older than this is treated as
    /// abandoned and taken over. defaults to the action default timeout for older callers.
    #[serde(default = "default_idempotency_lease_seconds")]
    pub lease_seconds: i64,
}

fn default_idempotency_lease_seconds() -> i64 {
    60
}

/// request body for releasing an unfinished reservation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IdempotencyReleaseRequest {
    pub scope: String,
    pub key: String,
    pub owner_node_run_id: Uuid,
}

/// request body for recording a completed execution against a reserved key.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IdempotencyCompleteRequest {
    pub scope: String,
    pub key: String,
    pub owner_node_run_id: Uuid,
    #[serde(default)]
    pub result: Value,
}

/// the stored replay payload for a completed action: enough to settle a redelivered node run
/// exactly as the original execution settled it, without re-invoking the provider.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct IdempotentActionResult {
    pub success: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub output_json: Option<Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OrchestrationEvent {
    pub event_id: Uuid,
    pub workflow_run_id: Uuid,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub workflow_node_run_id: Option<Uuid>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub node_id: Option<String>,
    pub event_type: String,
    #[serde(default)]
    pub payload: Value,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NewOrchestrationEvent {
    pub event_id: Uuid,
    pub workflow_run_id: Uuid,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub workflow_node_run_id: Option<Uuid>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub node_id: Option<String>,
    pub event_type: String,
    #[serde(default)]
    pub payload: Value,
    pub created_at: DateTime<Utc>,
}

impl NewOrchestrationEvent {
    pub fn new(
        workflow_run_id: Uuid,
        node_id: Option<String>,
        event_type: impl Into<String>,
        payload: Value,
    ) -> Self {
        Self {
            event_id: Uuid::now_v7(),
            workflow_run_id,
            workflow_node_run_id: None,
            node_id,
            event_type: event_type.into(),
            payload,
            created_at: Utc::now(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReadyNodeRecord {
    pub id: Uuid,
    pub source_event_id: Uuid,
    pub workflow_run_id: Uuid,
    pub node_id: String,
    pub status: WorkflowStatus,
    pub ready_at: DateTime<Utc>,
    pub attempts: i64,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub claimed_by: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub claimed_until: Option<DateTime<Utc>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub completed_at: Option<DateTime<Utc>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReadyNodeClaimRequest {
    pub scheduler_id: String,
    pub lease_until: DateTime<Utc>,
    #[serde(default)]
    pub limit: Option<i64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReadyNodeProcessRequest {
    pub scheduler_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub workflow_run_id: Option<Uuid>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub node_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub next_ready_at: Option<DateTime<Utc>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ActionDispatchClaimRequest {
    pub scheduler_id: String,
    pub lease_until: DateTime<Utc>,
    #[serde(default)]
    pub limit: Option<i64>,
}
