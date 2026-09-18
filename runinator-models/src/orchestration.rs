use std::collections::BTreeMap;

use crate::{types::RuninatorType, value::Value, workflows::WorkflowStatus};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::validation::{
    SHORT_TEXT_MAX, Validate, ValidationError, identifier, positive_limit, required_text,
};

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

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BudgetExhaustion {
    Fail,
    Pause,
    Terminate,
}

fn validate_json_pointer(pointer: &str) -> Result<(), String> {
    if pointer.is_empty() || pointer.starts_with('/') {
        Ok(())
    } else {
        Err(format!("'{pointer}' is not a JSON pointer"))
    }
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

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[derive(Default)]
pub enum DeliverySemantics {
    #[default]
    AtLeastOnce,
    Idempotent,
    Reconcilable,
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

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AdapterAuthenticationKind {
    Secrets,
    ExecutionProfile,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum AdapterAuthentication {
    Secrets {
        #[serde(default)]
        secret_bindings: BTreeMap<String, Uuid>,
    },
    ExecutionProfile {
        profile: crate::execution_profiles::ExecutionProfileBinding,
        required_labels: BTreeMap<String, String>,
        /// Scopes frozen from adapter-kind metadata when this revision is authored.
        #[serde(default)]
        required_scopes: Vec<String>,
    },
}

impl Default for AdapterAuthentication {
    fn default() -> Self {
        Self::Secrets {
            secret_bindings: BTreeMap::new(),
        }
    }
}

impl AdapterAuthentication {
    pub fn kind(&self) -> AdapterAuthenticationKind {
        match self {
            Self::Secrets { .. } => AdapterAuthenticationKind::Secrets,
            Self::ExecutionProfile { .. } => AdapterAuthenticationKind::ExecutionProfile,
        }
    }

    pub fn secret_bindings(&self) -> Option<&BTreeMap<String, Uuid>> {
        match self {
            Self::Secrets { secret_bindings } => Some(secret_bindings),
            Self::ExecutionProfile { .. } => None,
        }
    }

    pub fn execution_profile(&self) -> Option<&crate::execution_profiles::ExecutionProfileBinding> {
        match self {
            Self::ExecutionProfile { profile, .. } => Some(profile),
            Self::Secrets { .. } => None,
        }
    }
}

/// How an orchestration adapter obtains external events.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum AdapterTransport {
    #[default]
    Webhook,
    Polling,
}

impl AdapterTransport {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Webhook => "webhook",
            Self::Polling => "polling",
        }
    }
}

/// The narrowest identity widths every database backend can store. `scope` and `correlation_key`
/// both sit inside one exact unique key, which mysql caps at 3072 utf8mb4 bytes, so they are far
/// smaller than the payload limits around them. Keep these in step with the column widths in
/// `migrations/*/20260827000002_ingress_admissions.sql`.
pub const INGRESS_SOURCE_LIMIT: usize = 128;
pub const INGRESS_DELIVERY_ID_LIMIT: usize = 512;
pub const INGRESS_EVENT_TYPE_LIMIT: usize = 256;
pub const INGRESS_SCOPE_LIMIT: usize = 255;
pub const INGRESS_CORRELATION_KEY_LIMIT: usize = 255;

/// Validate an alternate correlation identity against the narrowest storage contract supported by
/// every database backend. Alias identities are intentionally smaller than arbitrary normalized
/// event identities because all three values participate in one unique lookup key.
pub fn validate_correlation_alias_identity(
    source: &str,
    scope: &str,
    correlation_key: &str,
) -> Result<(), String> {
    for (name, value, limit) in [
        ("source", source, 128usize),
        ("scope", scope, 255usize),
        ("correlation_key", correlation_key, 255usize),
    ] {
        if value.trim().is_empty() {
            return Err(format!("correlation alias {name} must not be empty"));
        }
        if value.len() > limit || value.chars().any(char::is_control) {
            return Err(format!("correlation alias {name} is not a valid identity"));
        }
    }
    Ok(())
}

/// The artifact kind currently owning a correlation-key admission.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum IngressTargetKind {
    Workflow,
    Pipeline,
}

/// Durable state of one correlation-key generation. The store owns its atomic transitions.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum IngressAdmissionStatus {
    Active,
    Terminal,
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
            setting_bindings: vec![],
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
            setting_bindings: vec![],
        };
        assert!(policy.validate().is_err());
    }

    #[test]
    fn orchestration_entry_must_be_a_declared_member_and_epoch_budget_is_positive() {
        let policy = OrchestrationPolicy {
            entry_member: Some("acme.mission.implement".into()),
            max_epochs: Some(3),
            ..Default::default()
        };
        assert!(policy.validate(["acme.mission.implement"]).is_ok());
        assert!(policy.validate(["acme.mission.review"]).is_err());

        let zero_budget = OrchestrationPolicy {
            max_epochs: Some(0),
            ..Default::default()
        };
        assert!(zero_budget.validate(["acme.mission.implement"]).is_err());
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
                    resolved_value: None,
                }],
                intent: None,
            }],
            setting_bindings: vec![],
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

    #[test]
    fn dispatch_routes_require_a_matching_orchestration_intent() {
        let policy = IngressPolicy {
            scope: "items".into(),
            routes: vec![IngressRoute {
                event_type: "changed".into(),
                lifecycle: IngressLifecycle::Active,
                action: IngressAction::Dispatch,
                predicates: vec![],
                intent: Some("refresh".into()),
            }],
            setting_bindings: vec![],
        };
        assert!(policy.validate_dispatches(None).is_err());

        let mut orchestration = OrchestrationPolicy::default();
        orchestration.intents.insert(
            "refresh".into(),
            IntentPolicy {
                effect: ControlEffect::Observe,
                priority: 10,
                coalesce_seconds: None,
                stop: EpochStopAction::Cancel,
                restart: RestartSelector::Entry,
                subject_revision_pointer: None,
                allow_self_originated: false,
                signal_name: None,
            },
        );
        assert!(policy.validate_dispatches(Some(&orchestration)).is_ok());

        orchestration.intents.clear();
        assert!(policy.validate_dispatches(Some(&orchestration)).is_err());
    }

    fn kind(kind: &str, scope_template: Option<&str>) -> AdapterKindMetadata {
        AdapterKindMetadata {
            kind: kind.into(),
            version: "1".into(),
            display_name: kind.into(),
            description: None,
            fields: vec![],
            polling_fields: vec![],
            event_names: vec![],
            canonical_pointers: vec![],
            capabilities: vec![],
            polling_authentication: vec![],
            polling_secret_fields: vec![],
            execution_profile_scopes: vec![],
            execution_profile_required_labels: Default::default(),
            identity_fields: vec![],
            scope_template: scope_template.map(str::to_owned),
            setup_instructions: vec![],
        }
    }

    fn scoped(scope: &str) -> IngressPolicy {
        IngressPolicy {
            scope: scope.into(),
            routes: vec![],
            setting_bindings: vec![],
        }
    }

    #[test]
    fn an_ingress_scope_no_adapter_can_emit_is_refused() {
        // the three cases that cost the harness bring-up a day: a mission-shaped scope against a
        // kind that only ever emits repository scopes, the repository scope that does reach, and a
        // kind whose whole scope is a placeholder and therefore reaches anything.
        let github = kind("github", Some("github:repository:{repository_id}"));
        let error = scoped("mission.flint.review")
            .validate_reachability(&[github.clone()])
            .expect_err("a mission scope is not a repository scope");
        assert!(error.contains("mission.flint.review"), "{error}");
        assert!(error.contains("github:repository:"), "{error}");

        assert!(
            scoped("github:repository:1242743236")
                .validate_reachability(&[github.clone()])
                .is_ok()
        );
        // the placeholder has to consume something: the bare prefix names no repository.
        assert!(
            scoped("github:repository:")
                .validate_reachability(&[github.clone()])
                .is_err()
        );
        assert!(
            scoped("mission.flint.review")
                .validate_reachability(&[github, kind("jira", Some("{routing_scope}"))])
                .is_ok()
        );
    }

    #[test]
    fn a_kind_that_declares_no_scope_template_never_blocks_an_apply() {
        // reachability is a guard, not a gate: a kind that has not described its scope says
        // nothing about what is reachable, and must not turn an apply into a refusal.
        assert!(
            scoped("anything at all")
                .validate_reachability(&[kind("generic_webhook", None)])
                .is_ok()
        );
        assert!(scoped("anything at all").validate_reachability(&[]).is_ok());
    }

    #[test]
    fn correlation_alias_identity_matches_the_cross_database_key_limits() {
        assert!(validate_correlation_alias_identity("github", "issues", "owner/repo#42").is_ok());
        assert!(validate_correlation_alias_identity("github", "issues", "\n").is_err());
        assert!(validate_correlation_alias_identity("github", "issues", &"x".repeat(256)).is_err());
    }
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

fn default_idempotency_lease_seconds() -> i64 {
    60
}

mod ingress_predicate;
pub use ingress_predicate::IngressPredicate;

mod ingress_route;
pub use ingress_route::IngressRoute;

mod ingress_policy;
pub use ingress_policy::IngressPolicy;

mod intent_policy;
pub use intent_policy::IntentPolicy;

mod result_mapping;
pub use result_mapping::ResultMapping;

mod workspace_policy;
pub use workspace_policy::WorkspacePolicy;

mod phase_policy;
pub use phase_policy::PhasePolicy;

mod budget_policy;
pub use budget_policy::BudgetPolicy;

mod orchestration_policy;
pub use orchestration_policy::OrchestrationPolicy;

mod orchestration_correlation_alias;
pub use orchestration_correlation_alias::OrchestrationCorrelationAlias;

mod orchestration_binding;
pub use orchestration_binding::OrchestrationBinding;

mod new_orchestration_binding;
pub use new_orchestration_binding::NewOrchestrationBinding;

mod orchestration_epoch;
pub use orchestration_epoch::OrchestrationEpoch;

mod orchestration_event_reduction;
pub use orchestration_event_reduction::OrchestrationEventReduction;

mod orchestration_pending_intent;
pub use orchestration_pending_intent::OrchestrationPendingIntent;

mod orchestration_command;
pub use orchestration_command::OrchestrationCommand;

mod orchestration_evidence;
pub use orchestration_evidence::OrchestrationEvidence;

mod external_operation;
pub use external_operation::ExternalOperation;

mod adapter_configuration_field;
pub use adapter_configuration_field::AdapterConfigurationField;

mod adapter_kind_metadata;
pub use adapter_kind_metadata::AdapterKindMetadata;

mod adapter_kind_catalog_entry;
pub use adapter_kind_catalog_entry::AdapterKindCatalogEntry;

mod adapter_definition;
pub use adapter_definition::AdapterDefinition;

mod adapter_revision;
pub use adapter_revision::AdapterRevision;

mod adapter_poll_status;
pub use adapter_poll_status::AdapterPollStatus;

mod normalized_adapter_event;
pub use normalized_adapter_event::NormalizedAdapterEvent;

mod ingress_event;
pub use ingress_event::IngressEvent;

mod ingress_target;
pub use ingress_target::IngressTarget;

mod ingress_admission;
pub use ingress_admission::IngressAdmission;

mod ingress_inbox_entry;
pub use ingress_inbox_entry::IngressInboxEntry;

mod ingress_event_record;
pub use ingress_event_record::IngressEventRecord;

mod ingress_promotion;
pub use ingress_promotion::IngressPromotion;

mod node_transition;
pub use node_transition::NodeTransition;

mod node_transition_stat;
pub use node_transition_stat::NodeTransitionStat;

mod external_item;
pub use external_item::ExternalItem;

mod approval_request;
pub use approval_request::ApprovalRequest;

mod gate;
pub use gate::Gate;

mod automation_event;
pub use automation_event::AutomationEvent;

mod catalog_item;
pub use catalog_item::CatalogItem;

mod idempotency_key;
pub use idempotency_key::IdempotencyKey;

mod idempotency_claim_request;
pub use idempotency_claim_request::IdempotencyClaimRequest;

mod idempotency_release_request;
pub use idempotency_release_request::IdempotencyReleaseRequest;

mod idempotency_complete_request;
pub use idempotency_complete_request::IdempotencyCompleteRequest;

mod idempotent_action_result;
pub use idempotent_action_result::IdempotentActionResult;

mod orchestration_event;
pub use orchestration_event::OrchestrationEvent;

mod new_orchestration_event;
pub use new_orchestration_event::NewOrchestrationEvent;

mod ready_node_record;
pub use ready_node_record::ReadyNodeRecord;

mod ready_node_claim_request;
pub use ready_node_claim_request::ReadyNodeClaimRequest;

mod ready_node_process_request;
pub use ready_node_process_request::ReadyNodeProcessRequest;

mod action_dispatch_claim_request;
pub use action_dispatch_claim_request::ActionDispatchClaimRequest;
