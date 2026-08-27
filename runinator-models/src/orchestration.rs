use crate::{value::Value, workflows::WorkflowStatus};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// The lifecycle state of a correlation-key admission when an ingress event arrives.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum IngressLifecycle {
    Unbound,
    Active,
    Terminal,
}

/// The provider-neutral disposition selected by an ingress policy.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum IngressAction {
    Start,
    Interrupt,
    Queue,
    Record,
    Requeue,
}

/// One static event-type route in a workflow or pipeline ingress policy.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct IngressRoute {
    pub event_type: String,
    pub lifecycle: IngressLifecycle,
    pub action: IngressAction,
}

/// Authored, provider-neutral policy carried in workflow/pipeline metadata.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct IngressPolicy {
    pub scope: String,
    #[serde(default)]
    pub routes: Vec<IngressRoute>,
}

impl IngressPolicy {
    /// Resolve the one policy action for an event in the admission's current lifecycle.
    /// A missing route intentionally means that the event is recorded nowhere and starts nothing.
    pub fn action_for(
        &self,
        event_type: &str,
        lifecycle: IngressLifecycle,
    ) -> Option<IngressAction> {
        self.routes
            .iter()
            .find(|route| route.event_type == event_type && route.lifecycle == lifecycle)
            .map(|route| route.action)
    }

    /// Validate the policy independently of any provider or target kind.
    pub fn validate(&self) -> Result<(), String> {
        if self.scope.trim().is_empty() {
            return Err("ingress scope must not be empty".into());
        }
        for route in &self.routes {
            if route.event_type.trim().is_empty() {
                return Err("ingress event type must not be empty".into());
            }
            if !route.action.is_allowed_when(route.lifecycle) {
                return Err(format!(
                    "ingress action '{}' is not valid when the admission is {}",
                    route.action.as_str(),
                    route.lifecycle.as_str()
                ));
            }
        }
        Ok(())
    }
}

impl IngressLifecycle {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Unbound => "unbound",
            Self::Active => "active",
            Self::Terminal => "terminal",
        }
    }
}

impl IngressAction {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Start => "start",
            Self::Interrupt => "interrupt",
            Self::Queue => "queue",
            Self::Record => "record",
            Self::Requeue => "requeue",
        }
    }

    pub fn is_allowed_when(self, lifecycle: IngressLifecycle) -> bool {
        matches!(
            (lifecycle, self),
            (IngressLifecycle::Unbound, Self::Start | Self::Record)
                | (
                    IngressLifecycle::Active,
                    Self::Interrupt | Self::Queue | Self::Record
                )
                | (IngressLifecycle::Terminal, Self::Requeue | Self::Record)
        )
    }
}

/// Opaque event accepted by the generic workflow/pipeline ingress surface.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IngressEvent {
    pub source: String,
    pub event_id: String,
    pub event_type: String,
    pub correlation_key: String,
    #[serde(default)]
    pub payload: Value,
    #[serde(default)]
    pub occurred_at: Option<DateTime<Utc>>,
}

/// The artifact kind currently owning a correlation-key admission.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum IngressTargetKind {
    Workflow,
    Pipeline,
}

/// Stable target identity retained with an admission generation.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct IngressTarget {
    pub kind: IngressTargetKind,
    pub id: Uuid,
}

/// Durable state of one correlation-key generation. The store owns its atomic transitions.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum IngressAdmissionStatus {
    Active,
    Terminal,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IngressAdmission {
    pub id: Option<Uuid>,
    #[serde(default)]
    pub org_id: Option<Uuid>,
    pub scope: String,
    pub correlation_key: String,
    pub generation: i64,
    pub target: IngressTarget,
    pub status: IngressAdmissionStatus,
    #[serde(default)]
    pub workflow_run_id: Option<Uuid>,
    #[serde(default)]
    pub pipeline_run_id: Option<Uuid>,
    #[serde(default)]
    pub policy: Value,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

/// Result of atomically creating the active admission for one `(org, scope, correlation key)`.
/// The caller that receives `Acquired` is the only one permitted to start a new target run.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "outcome", content = "admission", rename_all = "snake_case")]
pub enum IngressAdmissionClaim {
    Acquired(IngressAdmission),
    Existing(IngressAdmission),
}

/// Durable outcome of one provider-neutral ingress event.  The value is returned unchanged for
/// retries carrying the same `(source, event_id)`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum IngressEventDisposition {
    Started,
    Recorded,
    Queued,
    InterruptRequested,
    Requeued,
    Rejected,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum IngressQueueState {
    None,
    Queued,
    Claimed,
    Promoted,
}

/// One immutable event in an admission's ordered timeline.  Result references are filled as the
/// event starts (or is promoted into) a generation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IngressInboxEntry {
    pub id: Uuid,
    pub admission_id: Uuid,
    pub sequence: i64,
    pub generation: i64,
    pub source: String,
    pub event_id: String,
    pub event_type: String,
    pub correlation_key: String,
    pub payload: Value,
    pub occurred_at: Option<DateTime<Utc>>,
    pub received_at: DateTime<Utc>,
    pub disposition: IngressEventDisposition,
    pub queue_state: IngressQueueState,
    pub queue_position: Option<i64>,
    pub promoted_generation: Option<i64>,
    pub workflow_run_id: Option<Uuid>,
    pub pipeline_run_id: Option<Uuid>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IngressEventRecord {
    pub entry: IngressInboxEntry,
    pub duplicate: bool,
}

/// Atomic settlement result handed to the engine when the oldest queued event became the next
/// active generation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IngressPromotion {
    pub admission: IngressAdmission,
    pub event: IngressInboxEntry,
    pub claim_token: Uuid,
}

#[cfg(test)]
mod ingress_policy_tests {
    use super::*;

    #[test]
    fn resolves_routes_by_event_and_lifecycle() {
        let policy = IngressPolicy {
            scope: "issue.lifecycle".into(),
            routes: vec![IngressRoute {
                event_type: "changed".into(),
                lifecycle: IngressLifecycle::Active,
                action: IngressAction::Queue,
            }],
        };
        assert_eq!(
            policy.action_for("changed", IngressLifecycle::Active),
            Some(IngressAction::Queue)
        );
        assert_eq!(
            policy.action_for("changed", IngressLifecycle::Terminal),
            None
        );
    }

    #[test]
    fn rejects_invalid_lifecycle_action_pairs() {
        let policy = IngressPolicy {
            scope: "issue.lifecycle".into(),
            routes: vec![IngressRoute {
                event_type: "changed".into(),
                lifecycle: IngressLifecycle::Unbound,
                action: IngressAction::Interrupt,
            }],
        };
        assert!(policy.validate().is_err());
    }
}

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

/// how a gate is resolved: `manual` (opened/closed from the UI), `condition` (the reducer
/// auto-evaluates a rexrap boolean), or `external` (status set via the API by an outside system).
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
    /// the thread of control this wake belongs to. stamped onto the ready-node row so a run with
    /// fan-out can wake one branch without disturbing its siblings. `None` for a wake that predates
    /// cursor-keyed arming, which resolves by node id as before.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cursor_id: Option<Uuid>,
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
            cursor_id: None,
            event_type: event_type.into(),
            payload,
            created_at: Utc::now(),
        }
    }

    /// address this wake to one cursor.
    pub fn for_cursor(mut self, cursor_id: Uuid) -> Self {
        self.cursor_id = Some(cursor_id);
        self
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReadyNodeRecord {
    pub id: Uuid,
    pub source_event_id: Uuid,
    pub workflow_run_id: Uuid,
    pub node_id: String,
    /// the cursor this row wakes. `None` for rows armed before cursor-keyed wakes, which the reducer
    /// still resolves by node id.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cursor_id: Option<Uuid>,
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
