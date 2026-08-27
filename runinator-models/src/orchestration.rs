use std::collections::BTreeMap;

use crate::{types::RuninatorType, value::Value, workflows::WorkflowStatus};
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
    Dispatch,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum IngressPredicateOperator {
    Equal,
    NotEqual,
    In,
    Contains,
    Exists,
}

/// A deliberately bounded condition over a normalized event payload. Keeping this vocabulary in
/// the model prevents adapters from smuggling executable policy into the control plane.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct IngressPredicate {
    pub pointer: String,
    pub operator: IngressPredicateOperator,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub value: Option<Value>,
}

impl IngressPredicate {
    pub fn matches(&self, payload: &Value) -> bool {
        let actual = payload.pointer(&self.pointer);
        match self.operator {
            IngressPredicateOperator::Exists => actual.is_some(),
            IngressPredicateOperator::Equal => actual == self.value.as_ref(),
            IngressPredicateOperator::NotEqual => actual != self.value.as_ref(),
            IngressPredicateOperator::In => self
                .value
                .as_ref()
                .and_then(Value::as_array)
                .is_some_and(|values| actual.is_some_and(|actual| values.contains(actual))),
            IngressPredicateOperator::Contains => match (actual, self.value.as_ref()) {
                (Some(Value::Array(values)), Some(expected)) => values.contains(expected),
                (Some(Value::String(value)), Some(Value::String(expected))) => {
                    value.contains(expected)
                }
                (Some(Value::Object(values)), Some(Value::String(expected))) => {
                    values.contains_key(expected)
                }
                _ => false,
            },
        }
    }

    pub fn validate(&self) -> Result<(), String> {
        if !self.pointer.is_empty() && !self.pointer.starts_with('/') {
            return Err(format!(
                "ingress predicate pointer '{}' must be empty or start with '/'",
                self.pointer
            ));
        }
        match self.operator {
            IngressPredicateOperator::Exists if self.value.is_some() => {
                Err("an exists predicate must not have a comparison value".into())
            }
            IngressPredicateOperator::Exists => Ok(()),
            _ if self.value.is_none() => Err("an ingress predicate requires a value".into()),
            IngressPredicateOperator::In if !self.value.as_ref().is_some_and(Value::is_array) => {
                Err("an in predicate requires an array value".into())
            }
            _ => Ok(()),
        }
    }
}

/// One static event-type route in a workflow or pipeline ingress policy.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct IngressRoute {
    pub event_type: String,
    pub lifecycle: IngressLifecycle,
    pub action: IngressAction,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub predicates: Vec<IngressPredicate>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub intent: Option<String>,
}

/// Authored, provider-neutral policy carried in workflow/pipeline metadata.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
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

    /// Resolve an action while honoring every predicate on the route. Callers handling a concrete
    /// event must use this form; `action_for` remains for compatibility and policy introspection.
    pub fn action_for_payload(
        &self,
        event_type: &str,
        lifecycle: IngressLifecycle,
        payload: &Value,
    ) -> Option<IngressAction> {
        self.routes_for_payload(event_type, lifecycle, payload)
            .into_iter()
            .next()
            .map(|route| route.action)
    }

    /// Return each route matching the concrete event in author order. Orchestration evaluates all
    /// dispatch matches; legacy one-action ingress behavior consumes the first match.
    pub fn routes_for_payload<'a>(
        &'a self,
        event_type: &str,
        lifecycle: IngressLifecycle,
        payload: &Value,
    ) -> Vec<&'a IngressRoute> {
        self.routes
            .iter()
            .filter(|route| {
                route.event_type == event_type
                    && route.lifecycle == lifecycle
                    && route
                        .predicates
                        .iter()
                        .all(|predicate| predicate.matches(payload))
            })
            .collect()
    }

    /// Return every matching named intent. The orchestration policy owns precedence; ingress only
    /// identifies candidates and never chooses control outcomes itself.
    pub fn dispatches_for(
        &self,
        event_type: &str,
        lifecycle: IngressLifecycle,
        payload: &Value,
    ) -> Vec<&str> {
        self.routes_for_payload(event_type, lifecycle, payload)
            .into_iter()
            .filter(|route| route.action == IngressAction::Dispatch)
            .filter_map(|route| route.intent.as_deref())
            .collect()
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
            for predicate in &route.predicates {
                predicate.validate()?;
            }
            match route.action {
                IngressAction::Dispatch
                    if route
                        .intent
                        .as_deref()
                        .is_none_or(|intent| intent.trim().is_empty()) =>
                {
                    return Err("a dispatch ingress route requires a non-empty intent".into());
                }
                IngressAction::Dispatch => {}
                _ if route.intent.is_some() => {
                    return Err("only a dispatch ingress route may name an intent".into());
                }
                _ => {}
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
            Self::Dispatch => "dispatch",
        }
    }

    pub fn is_allowed_when(self, lifecycle: IngressLifecycle) -> bool {
        matches!(
            (lifecycle, self),
            (IngressLifecycle::Unbound, Self::Start | Self::Record)
                | (
                    IngressLifecycle::Active,
                    Self::Interrupt | Self::Queue | Self::Record | Self::Dispatch
                )
                | (IngressLifecycle::Terminal, Self::Requeue | Self::Record)
        )
    }
}

/// Binding-level status. It is intentionally distinct from workflow and pipeline run status: one
/// binding can survive many immutable execution epochs.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OrchestrationStatus {
    Pending,
    Running,
    Waiting,
    Suspended,
    Completed,
    Failed,
    Terminated,
}

impl OrchestrationStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Pending => "pending",
            Self::Running => "running",
            Self::Waiting => "waiting",
            Self::Suspended => "suspended",
            Self::Completed => "completed",
            Self::Failed => "failed",
            Self::Terminated => "terminated",
        }
    }

    pub fn is_terminal(self) -> bool {
        matches!(self, Self::Completed | Self::Failed | Self::Terminated)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ControlEffect {
    Terminate,
    Suspend,
    Resume,
    Supersede,
    Observe,
    Signal,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case", tag = "kind", content = "member")]
pub enum RestartSelector {
    #[default]
    Entry,
    Current,
    Member(String),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum EpochStopAction {
    Pause,
    #[default]
    Cancel,
    None,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct IntentPolicy {
    pub effect: ControlEffect,
    pub priority: i32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub coalesce_seconds: Option<u64>,
    #[serde(default)]
    pub stop: EpochStopAction,
    #[serde(default)]
    pub restart: RestartSelector,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub subject_revision_pointer: Option<String>,
    #[serde(default)]
    pub allow_self_originated: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub signal_name: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
pub struct ResultMapping {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub subject_revision: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub resources: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub evidence: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub failure_class: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct WorkspacePolicy {
    pub scope: String,
    #[serde(default)]
    pub requirements: Value,
    #[serde(default = "default_workspace_lease_seconds")]
    pub lease_seconds: u64,
    #[serde(default)]
    pub reuse: bool,
    #[serde(default)]
    pub recovery: WorkspaceRecovery,
}

fn default_workspace_lease_seconds() -> u64 {
    300
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum WorkspaceRecovery {
    #[default]
    Replace,
    Wait,
    Fail,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
pub struct PhasePolicy {
    #[serde(default)]
    pub result: ResultMapping,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub workspace: Option<WorkspacePolicy>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BudgetExhaustion {
    Fail,
    Pause,
    Terminate,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BudgetPolicy {
    pub attempts: u32,
    pub exhausted: BudgetExhaustion,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
pub struct OrchestrationPolicy {
    #[serde(default)]
    pub intents: BTreeMap<String, IntentPolicy>,
    #[serde(default)]
    pub phases: BTreeMap<String, PhasePolicy>,
    #[serde(default)]
    pub budgets: BTreeMap<String, BudgetPolicy>,
    #[serde(default)]
    pub defaults: Value,
}

impl OrchestrationPolicy {
    pub fn validate<'a>(
        &self,
        member_keys: impl IntoIterator<Item = &'a str>,
    ) -> Result<(), String> {
        let member_keys = member_keys
            .into_iter()
            .collect::<std::collections::BTreeSet<_>>();
        let mut priorities = std::collections::BTreeMap::new();
        for (name, intent) in &self.intents {
            if name.trim().is_empty() {
                return Err("orchestration intent names must not be empty".into());
            }
            if let Some(existing) = priorities.insert(intent.priority, name) {
                return Err(format!(
                    "orchestration intents '{existing}' and '{name}' use the same priority {}",
                    intent.priority
                ));
            }
            if let RestartSelector::Member(member) = &intent.restart
                && !member_keys.contains(member.as_str())
            {
                return Err(format!(
                    "orchestration restart member '{member}' does not exist"
                ));
            }
            if let Some(pointer) = &intent.subject_revision_pointer {
                validate_json_pointer(pointer)?;
            }
        }
        for (member, phase) in &self.phases {
            if !member_keys.contains(member.as_str()) {
                return Err(format!(
                    "orchestration phase member '{member}' does not exist"
                ));
            }
            for pointer in [
                &phase.result.subject_revision,
                &phase.result.resources,
                &phase.result.evidence,
                &phase.result.failure_class,
            ]
            .into_iter()
            .flatten()
            {
                validate_json_pointer(pointer)?;
            }
            if let Some(workspace) = &phase.workspace
                && workspace.scope.trim().is_empty()
            {
                return Err(format!(
                    "workspace scope for phase '{member}' must not be empty"
                ));
            }
        }
        for (name, budget) in &self.budgets {
            if name.trim().is_empty() || budget.attempts == 0 {
                return Err("budget names must be non-empty and attempts must be positive".into());
            }
        }
        Ok(())
    }
}

fn validate_json_pointer(pointer: &str) -> Result<(), String> {
    if pointer.is_empty() || pointer.starts_with('/') {
        Ok(())
    } else {
        Err(format!("'{pointer}' is not a JSON pointer"))
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OrchestrationBinding {
    pub id: Uuid,
    pub admission_id: Uuid,
    #[serde(default)]
    pub org_id: Option<Uuid>,
    pub scope: String,
    pub correlation_key: String,
    pub generation: i64,
    pub pipeline_id: Uuid,
    pub pipeline_revision: i64,
    pub pipeline_digest: String,
    #[serde(default)]
    pub adapter_id: Option<Uuid>,
    #[serde(default)]
    pub adapter_revision: Option<i64>,
    pub policy: OrchestrationPolicy,
    pub status: OrchestrationStatus,
    #[serde(default)]
    pub current_phase: Option<String>,
    pub current_attempt: i64,
    pub current_epoch: i64,
    #[serde(default)]
    pub restart_member: Option<String>,
    #[serde(default)]
    pub resume_existing_epoch: bool,
    #[serde(default)]
    pub subject_revision: Option<String>,
    #[serde(default)]
    pub resources: Value,
    #[serde(default)]
    pub budgets: BTreeMap<String, u32>,
    pub last_reduced_sequence: i64,
    pub version: i64,
    #[serde(default)]
    pub reducer_lease_owner: Option<String>,
    #[serde(default)]
    pub reducer_leased_until: Option<DateTime<Utc>>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    #[serde(default)]
    pub finished_at: Option<DateTime<Utc>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NewOrchestrationBinding {
    pub id: Uuid,
    pub admission_id: Uuid,
    pub org_id: Option<Uuid>,
    pub scope: String,
    pub correlation_key: String,
    pub generation: i64,
    pub pipeline_id: Uuid,
    pub pipeline_revision: i64,
    pub pipeline_digest: String,
    pub adapter_id: Option<Uuid>,
    pub adapter_revision: Option<i64>,
    pub policy: OrchestrationPolicy,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OrchestrationEpoch {
    pub id: Uuid,
    pub binding_id: Uuid,
    pub epoch: i64,
    #[serde(default)]
    pub pipeline_run_id: Option<Uuid>,
    #[serde(default)]
    pub start_member: Option<String>,
    #[serde(default)]
    pub parameters: Value,
    pub status: String,
    pub reason: String,
    pub created_at: DateTime<Utc>,
    #[serde(default)]
    pub started_at: Option<DateTime<Utc>>,
    #[serde(default)]
    pub finished_at: Option<DateTime<Utc>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OrchestrationEventReduction {
    pub id: Uuid,
    pub binding_id: Uuid,
    pub inbox_event_id: Uuid,
    pub sequence: i64,
    #[serde(default)]
    pub matched_intents: Vec<String>,
    #[serde(default)]
    pub winner: Option<String>,
    #[serde(default)]
    pub suppressed_intents: Vec<String>,
    pub binding_version: i64,
    pub disposition: String,
    #[serde(default)]
    pub detail: Value,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OrchestrationPendingIntent {
    pub id: Uuid,
    pub binding_id: Uuid,
    pub intent: String,
    pub priority: i32,
    pub source_event_ids: Vec<Uuid>,
    #[serde(default)]
    pub latest_payload: Value,
    pub wake_at: DateTime<Utc>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OrchestrationCommandStatus {
    Pending,
    Claimed,
    Succeeded,
    Failed,
    Superseded,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OrchestrationCommand {
    pub id: Uuid,
    pub binding_id: Uuid,
    pub epoch: i64,
    pub command_type: String,
    pub operation_key: String,
    #[serde(default)]
    pub payload: Value,
    pub status: OrchestrationCommandStatus,
    pub attempts: i64,
    #[serde(default)]
    pub claimed_by: Option<String>,
    #[serde(default)]
    pub claimed_until: Option<DateTime<Utc>>,
    #[serde(default)]
    pub result: Value,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OrchestrationEvidence {
    pub id: Uuid,
    pub binding_id: Uuid,
    #[serde(default)]
    pub epoch: Option<i64>,
    pub kind: String,
    #[serde(default)]
    pub subject_revision: Option<String>,
    #[serde(default)]
    pub payload: Value,
    #[serde(default)]
    pub source_event_id: Option<Uuid>,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DeliverySemantics {
    AtLeastOnce,
    Idempotent,
    Reconcilable,
}

impl Default for DeliverySemantics {
    fn default() -> Self {
        Self::AtLeastOnce
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ExternalOperationStatus {
    Pending,
    Running,
    Waiting,
    Succeeded,
    Failed,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExternalOperation {
    pub id: Uuid,
    pub binding_id: Uuid,
    /// Immutable execution coordinates used to reject stale receipts and operator retries.
    pub epoch: i64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub workflow_run_id: Option<Uuid>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub effect_id: Option<Uuid>,
    pub operation_key: String,
    pub provider: String,
    pub action: String,
    pub semantics: DeliverySemantics,
    pub attempt: i64,
    pub status: ExternalOperationStatus,
    pub ambiguous: bool,
    #[serde(default)]
    pub provenance: Value,
    #[serde(default)]
    pub receipt: Value,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AdapterConfigurationField {
    pub name: String,
    pub value_type: RuninatorType,
    #[serde(default)]
    pub required: bool,
    #[serde(default)]
    pub secret: bool,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub default: Value,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AdapterKindMetadata {
    pub kind: String,
    pub version: String,
    pub display_name: String,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub fields: Vec<AdapterConfigurationField>,
    #[serde(default)]
    pub event_names: Vec<String>,
    #[serde(default)]
    pub canonical_pointers: Vec<String>,
    #[serde(default)]
    pub capabilities: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AdapterKindCatalogEntry {
    pub metadata: AdapterKindMetadata,
    pub origin: String,
    pub healthy: bool,
    #[serde(default)]
    pub error: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AdapterDefinition {
    pub id: Uuid,
    pub org_id: Uuid,
    pub name: String,
    pub kind: String,
    pub current_revision: i64,
    pub enabled: bool,
    pub endpoint_identity: String,
    pub has_admitted_binding: bool,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AdapterRevision {
    pub id: Uuid,
    pub adapter_id: Uuid,
    pub revision: i64,
    pub kind_version: String,
    #[serde(default)]
    pub configuration: Value,
    #[serde(default)]
    pub secret_bindings: BTreeMap<String, Uuid>,
    #[serde(default)]
    pub identity_configuration: Value,
    pub created_at: DateTime<Utc>,
    #[serde(default)]
    pub actor_id: Option<Uuid>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NormalizedAdapterEvent {
    pub source: String,
    pub delivery_id: String,
    pub event_type: String,
    pub scope: String,
    pub correlation_key: String,
    #[serde(default)]
    pub subject_revision: Option<String>,
    #[serde(default)]
    pub occurred_at: Option<DateTime<Utc>>,
    #[serde(default)]
    pub payload: Value,
    #[serde(default)]
    pub provenance: Value,
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
    pub provenance: Value,
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
    #[serde(default)]
    pub provenance: Value,
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
                predicates: vec![],
                intent: None,
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
                predicates: vec![],
                intent: None,
            }],
        };
        assert!(policy.validate().is_err());
    }

    #[test]
    fn concrete_event_actions_honor_route_predicates() {
        let policy = IngressPolicy {
            scope: "items".into(),
            routes: vec![IngressRoute {
                event_type: "changed".into(),
                lifecycle: IngressLifecycle::Unbound,
                action: IngressAction::Start,
                predicates: vec![IngressPredicate {
                    pointer: "/labels".into(),
                    operator: IngressPredicateOperator::Contains,
                    value: Some(Value::String("auto".into())),
                }],
                intent: None,
            }],
        };
        assert_eq!(
            policy.action_for_payload(
                "changed",
                IngressLifecycle::Unbound,
                &crate::json!({ "labels": ["auto"] }),
            ),
            Some(IngressAction::Start)
        );
        assert_eq!(
            policy.action_for_payload(
                "changed",
                IngressLifecycle::Unbound,
                &crate::json!({ "labels": ["manual"] }),
            ),
            None
        );
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
