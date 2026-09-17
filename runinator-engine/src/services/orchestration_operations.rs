//! Generic correlated-orchestration admission and deterministic intent reduction.

use std::sync::Arc;

use chrono::{Duration, Utc};
use runinator_models::{
    errors::SendableError,
    orchestration::{
        AdapterDefinition, ControlEffect, EpochStopAction, ExternalOperation, IngressAdmission,
        IngressEvent, IngressEventDisposition, IngressEventRecord, IngressInboxEntry,
        IngressLifecycle, IngressPolicy, NewOrchestrationBinding, OrchestrationBinding,
        OrchestrationCommand, OrchestrationCorrelationAlias, OrchestrationEpoch,
        OrchestrationEventReduction, OrchestrationEvidence, OrchestrationPendingIntent,
        OrchestrationPolicy, OrchestrationStatus, RestartSelector, WorkspaceRecovery,
        validate_correlation_alias_identity,
    },
    pipelines::Pipeline,
    value::Value,
};
use runinator_store::{
    RuntimeStore,
    roles::{
        DefinitionStore, ExternalOperationUpdate, IngressStore, NewOrchestrationCommand,
        NewOrchestrationCorrelationAlias, NewOrchestrationEpoch, OrchestrationBindingFilter,
        OrchestrationBindingUpdate, OrchestrationStore, WorkflowVmStore, WorkspaceStore,
    },
};
use uuid::Uuid;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum IntentApplyOutcome {
    Applied,
    IgnoredState,
    IgnoredSubjectRevision,
    IgnoredNoActiveMember,
}

impl IntentApplyOutcome {
    fn as_str(self) -> &'static str {
        match self {
            Self::Applied => "applied",
            Self::IgnoredState => "ignored_state",
            Self::IgnoredSubjectRevision => "ignored_subject_revision",
            Self::IgnoredNoActiveMember => "ignored_no_active_member",
        }
    }
}

/// Resolve named-intent precedence without knowing any provider or problem-domain vocabulary.
pub fn choose_intent<'a>(
    candidates: impl IntoIterator<Item = &'a str>,
    policy: &OrchestrationPolicy,
) -> IntentDecision {
    let mut matched = candidates
        .into_iter()
        .filter(|name| policy.intents.contains_key(*name))
        .map(str::to_owned)
        .collect::<Vec<_>>();
    matched.sort();
    matched.dedup();
    matched.sort_by(|left, right| {
        policy.intents[right]
            .priority
            .cmp(&policy.intents[left].priority)
            .then_with(|| left.cmp(right))
    });
    let winner = matched.first().cloned();
    let suppressed = matched.iter().skip(1).cloned().collect();
    IntentDecision {
        matched,
        winner,
        suppressed,
    }
}

fn is_active_harnessed_ai_effect(effect: &runinator_models::workflow_vm::WorkflowEffect) -> bool {
    if !matches!(
        effect.status,
        runinator_models::workflow_vm::WorkflowEffectStatus::Running
            | runinator_models::workflow_vm::WorkflowEffectStatus::InputRequired
    ) {
        return false;
    }
    matches!(
        &effect.request,
        runinator_models::workflow_vm::WorkflowEffectRequest::Action {
            provider,
            function,
            input,
            ..
        } if provider == "ai-command"
            && matches!(function.as_str(), "claude_code" | "codex")
            && input.get("harnessed").and_then(Value::as_bool) == Some(true)
    )
}

fn resolve_restart_member(
    binding: &OrchestrationBinding,
    selector: &RestartSelector,
) -> Option<String> {
    match selector {
        RestartSelector::Entry => binding.policy.entry_member.clone(),
        RestartSelector::Current => binding.current_phase.clone(),
        RestartSelector::Member(member) => Some(member.clone()),
    }
}

fn signal_revision_matches(expected: Option<&str>, payload: &Value, pointer: &str) -> bool {
    expected
        .is_some_and(|expected| payload.pointer(pointer).and_then(Value::as_str) == Some(expected))
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use super::*;
    use runinator_models::orchestration::{IntentPolicy, OrchestrationPolicy};

    #[test]
    fn precedence_is_priority_driven_and_suppressed_is_stable() {
        let mut intents = BTreeMap::new();
        for (name, effect, priority) in [
            ("audit", ControlEffect::Observe, 10),
            ("redo", ControlEffect::Supersede, 80),
            ("stop", ControlEffect::Terminate, 100),
        ] {
            intents.insert(
                name.into(),
                IntentPolicy {
                    effect,
                    priority,
                    coalesce_seconds: None,
                    stop: Default::default(),
                    restart: Default::default(),
                    subject_revision_pointer: None,
                    allow_self_originated: false,
                    signal_name: None,
                },
            );
        }
        let policy = OrchestrationPolicy {
            intents,
            ..Default::default()
        };
        assert_eq!(
            choose_intent(["audit", "stop", "redo"], &policy),
            IntentDecision {
                matched: vec!["stop".into(), "redo".into(), "audit".into()],
                winner: Some("stop".into()),
                suppressed: vec!["redo".into(), "audit".into()],
            }
        );
    }

    #[test]
    fn revision_bound_signals_require_both_revisions_to_exist_and_match() {
        let payload = runinator_models::json!({ "revision": "r2" });
        assert!(signal_revision_matches(Some("r2"), &payload, "/revision"));
        assert!(!signal_revision_matches(None, &payload, "/revision"));
        assert!(!signal_revision_matches(None, &Value::Null, "/revision"));
        assert!(!signal_revision_matches(Some("r1"), &payload, "/revision"));
        assert!(!signal_revision_matches(Some("r2"), &payload, "/missing"));
    }

    #[test]
    fn mission_steering_targets_only_an_active_harnessed_ai_effect() {
        use runinator_models::workflow_vm::{
            WORKFLOW_EFFECT_PROTOCOL_VERSION, WorkflowEffect, WorkflowEffectRequest,
            WorkflowEffectStatus,
        };

        let now = Utc::now().timestamp();
        let mut effect = WorkflowEffect {
            version: WORKFLOW_EFFECT_PROTOCOL_VERSION,
            id: Uuid::now_v7(),
            workflow_run_id: Uuid::now_v7(),
            continuation_id: Uuid::now_v7(),
            sequence: 1,
            attempt: 0,
            node_id: Some("claude_implementation".into()),
            timeline_category: Default::default(),
            request: WorkflowEffectRequest::Action {
                provider: "ai-command".into(),
                function: "claude_code".into(),
                input: runinator_models::json!({ "harnessed": true }),
                timeout_seconds: Some(60),
                retry: Default::default(),
                tags: Vec::new(),
                required_labels: Default::default(),
                workspace_affinity: None,
                execution_profile: None,
                idempotency_key: None,
                function_binding: None,
            },
            status: WorkflowEffectStatus::Running,
            current_executor_replica_id: Some(Uuid::now_v7()),
            last_executor_replica_id: None,
            result: None,
            message: None,
            created_at: now,
            updated_at: now,
            finished_at: None,
        };
        assert!(is_active_harnessed_ai_effect(&effect));

        effect.status = WorkflowEffectStatus::Succeeded;
        assert!(!is_active_harnessed_ai_effect(&effect));
        effect.status = WorkflowEffectStatus::Running;
        let WorkflowEffectRequest::Action { input, .. } = &mut effect.request else {
            unreachable!();
        };
        *input = runinator_models::json!({ "harnessed": false });
        assert!(!is_active_harnessed_ai_effect(&effect));
        let WorkflowEffectRequest::Action {
            function, input, ..
        } = &mut effect.request
        else {
            unreachable!();
        };
        *function = "codex".into();
        *input = runinator_models::json!({ "harnessed": true });
        assert!(is_active_harnessed_ai_effect(&effect));
    }

    #[tokio::test]
    async fn self_originated_echo_is_evidence_without_dispatch_by_default() {
        use runinator_database::sqlite::SqliteDb;
        use runinator_models::{
            orchestration::{
                DeliverySemantics, ExternalOperation, ExternalOperationStatus, IngressAction,
                IngressAdmissionStatus, IngressEvent, IngressEventDisposition, IngressRoute,
                IngressTarget, IngressTargetKind,
            },
            pipelines::{Pipeline, PipelineGraph},
        };
        use runinator_store::prelude::*;

        let path =
            std::env::temp_dir().join(format!("runinator-self-originated-{}.db", Uuid::now_v7()));
        let db = Arc::new(SqliteDb::new(path.to_str().unwrap()).await.unwrap());
        db.run_init_scripts(&Vec::new()).await.unwrap();
        let pipeline = db
            .upsert_pipeline(&Pipeline {
                id: None,
                name: "self-origin test".into(),
                key: None,
                namespace: None,
                description: None,
                org_id: None,
                enabled: true,
                graph: PipelineGraph {
                    version: runinator_models::pipelines::PIPELINE_GRAPH_VERSION,
                    ..Default::default()
                },
                concurrency: Default::default(),
                defaults: Default::default(),
                metadata: Value::Null,
                created_at: None,
                updated_at: None,
            })
            .await
            .unwrap();
        let pipeline_id = pipeline.id.unwrap();
        let ingress = IngressPolicy {
            scope: "objects".into(),
            routes: vec![
                IngressRoute {
                    event_type: "created".into(),
                    lifecycle: IngressLifecycle::Unbound,
                    action: IngressAction::Start,
                    predicates: vec![],
                    intent: None,
                },
                IngressRoute {
                    event_type: "updated".into(),
                    lifecycle: IngressLifecycle::Active,
                    action: IngressAction::Dispatch,
                    predicates: vec![],
                    intent: Some("stop".into()),
                },
                IngressRoute {
                    event_type: "scope_changed".into(),
                    lifecycle: IngressLifecycle::Active,
                    action: IngressAction::Dispatch,
                    predicates: vec![],
                    intent: Some("rework".into()),
                },
                IngressRoute {
                    event_type: "out_of_band_override".into(),
                    lifecycle: IngressLifecycle::Active,
                    action: IngressAction::Dispatch,
                    predicates: vec![],
                    intent: Some("stop".into()),
                },
            ],
            setting_bindings: vec![],
        };
        let now = Utc::now();
        let admission = match db
            .claim_ingress_admission(
                IngressAdmission {
                    id: Some(Uuid::now_v7()),
                    org_id: None,
                    scope: ingress.scope.clone(),
                    correlation_key: "object-1".into(),
                    generation: 1,
                    target: IngressTarget {
                        kind: IngressTargetKind::Pipeline,
                        id: pipeline_id,
                    },
                    status: IngressAdmissionStatus::Active,
                    workflow_run_id: None,
                    pipeline_run_id: None,
                    policy: serde_json::to_value(&ingress).unwrap().into(),
                    created_at: now,
                    updated_at: now,
                },
                Some(IngressEvent {
                    source: "adapter:test".into(),
                    event_id: "created".into(),
                    event_type: "created".into(),
                    correlation_key: "object-1".into(),
                    payload: Value::Null,
                    provenance: Value::Null,
                    occurred_at: Some(now),
                }),
            )
            .await
            .unwrap()
        {
            runinator_models::orchestration::IngressAdmissionClaim::Acquired(value) => value,
            _ => panic!("admission must be acquired"),
        };
        let mut policy = OrchestrationPolicy::default();
        policy.intents.insert(
            "stop".into(),
            IntentPolicy {
                effect: ControlEffect::Terminate,
                priority: 100,
                coalesce_seconds: None,
                stop: Default::default(),
                restart: Default::default(),
                subject_revision_pointer: None,
                allow_self_originated: false,
                signal_name: None,
            },
        );
        policy.intents.insert(
            "rework".into(),
            IntentPolicy {
                effect: ControlEffect::Supersede,
                priority: 80,
                coalesce_seconds: Some(300),
                stop: Default::default(),
                restart: Default::default(),
                subject_revision_pointer: None,
                allow_self_originated: false,
                signal_name: None,
            },
        );
        let binding = db
            .create_orchestration_binding(NewOrchestrationBinding {
                id: Uuid::now_v7(),
                admission_id: admission.id.unwrap(),
                org_id: None,
                scope: admission.scope.clone(),
                correlation_key: admission.correlation_key.clone(),
                generation: 1,
                pipeline_id,
                pipeline_revision: 1,
                pipeline_digest: "test".into(),
                adapter_id: None,
                adapter_revision: None,
                policy,
            })
            .await
            .unwrap();
        db.create_external_operation(ExternalOperation {
            id: Uuid::now_v7(),
            binding_id: binding.id,
            epoch: 1,
            workflow_run_id: None,
            effect_id: None,
            operation_key: "effect-key".into(),
            provider: "example".into(),
            action: "ensure".into(),
            semantics: DeliverySemantics::Reconcilable,
            attempt: 1,
            status: ExternalOperationStatus::Succeeded,
            ambiguous: false,
            provenance: runinator_models::json!({
                "provider_idempotency_key": "provider-key"
            }),
            receipt: Value::Null,
            created_at: now,
            updated_at: now,
        })
        .await
        .unwrap();
        db.record_ingress_event(
            binding.admission_id,
            binding.generation,
            IngressEvent {
                source: "adapter:test".into(),
                event_id: "echo".into(),
                event_type: "updated".into(),
                correlation_key: binding.correlation_key.clone(),
                payload: Value::Null,
                provenance: runinator_models::json!({ "operation_key": "provider-key" }),
                occurred_at: Some(now),
            },
            IngressEventDisposition::Recorded,
            false,
            now,
        )
        .await
        .unwrap();
        let claimed = db
            .claim_orchestration_bindings(
                "self-origin-reducer".into(),
                now,
                now + Duration::minutes(1),
                1,
            )
            .await
            .unwrap()
            .pop()
            .unwrap();
        let reduced = OrchestrationOperations::new(db.clone())
            .reduce_binding(claimed, "self-origin-reducer")
            .await
            .unwrap();
        assert_eq!(reduced.status, OrchestrationStatus::Running);
        let reductions = db.fetch_orchestration_reductions(binding.id).await.unwrap();
        assert_eq!(reductions.last().unwrap().disposition, "self_originated");
        assert!(reductions.last().unwrap().winner.is_none());
        let evidence = db.fetch_orchestration_evidence(binding.id).await.unwrap();
        assert!(
            evidence
                .iter()
                .any(|item| item.kind == "self_originated_event")
        );
        assert_eq!(
            reductions
                .last()
                .unwrap()
                .detail
                .pointer("/event/event_id")
                .and_then(Value::as_str),
            Some("echo")
        );
        assert_eq!(
            reductions
                .last()
                .unwrap()
                .detail
                .pointer("/matched_routes/0/intent")
                .and_then(Value::as_str),
            Some("stop")
        );

        db.record_ingress_event(
            binding.admission_id,
            binding.generation,
            IngressEvent {
                source: "adapter:test".into(),
                event_id: "scope-change".into(),
                event_type: "scope_changed".into(),
                correlation_key: binding.correlation_key.clone(),
                payload: runinator_models::json!({ "revision": "r2" }),
                provenance: Value::Null,
                occurred_at: Some(now),
            },
            IngressEventDisposition::Recorded,
            false,
            now,
        )
        .await
        .unwrap();
        let reduced = OrchestrationOperations::new(db.clone())
            .reduce_binding(reduced, "self-origin-reducer")
            .await
            .unwrap();
        let pending = db
            .fetch_orchestration_pending_intents(binding.id)
            .await
            .unwrap();
        assert_eq!(pending.len(), 1);
        assert_eq!(pending[0].intent, "rework");
        let commands = db.fetch_orchestration_commands(binding.id).await.unwrap();
        let wake = commands
            .iter()
            .find(|command| command.command_type == "arm_intent_wake")
            .expect("coalescing should arm the broker waker through the command outbox");
        assert_eq!(
            wake.payload.get("intent").and_then(Value::as_str),
            Some("rework")
        );

        db.record_ingress_event(
            binding.admission_id,
            binding.generation,
            IngressEvent {
                source: "adapter:test".into(),
                event_id: "scope-change-newer".into(),
                event_type: "scope_changed".into(),
                correlation_key: binding.correlation_key.clone(),
                payload: runinator_models::json!({ "revision": "r3" }),
                provenance: Value::Null,
                occurred_at: Some(now),
            },
            IngressEventDisposition::Recorded,
            false,
            now,
        )
        .await
        .unwrap();
        let reduced = OrchestrationOperations::new(db.clone())
            .reduce_binding(reduced, "self-origin-reducer")
            .await
            .unwrap();
        let pending = db
            .fetch_orchestration_pending_intents(binding.id)
            .await
            .unwrap();
        assert_eq!(pending.len(), 1);
        assert_eq!(pending[0].source_event_ids.len(), 2);
        assert_eq!(
            pending[0]
                .latest_payload
                .get("revision")
                .and_then(Value::as_str),
            Some("r3")
        );
        assert_eq!(
            db.fetch_orchestration_commands(binding.id)
                .await
                .unwrap()
                .iter()
                .filter(|command| command.command_type == "arm_intent_wake")
                .count(),
            2
        );

        let operations = OrchestrationOperations::new(db.clone());
        let override_record = operations
            .record_out_of_band_override(
                &reduced,
                OutOfBandOverrideRequest {
                    target_kind: "pipeline_run".into(),
                    target_id: Uuid::now_v7(),
                    action: "pause".into(),
                    reason: "recover inconsistent provider state".into(),
                    idempotency_key: "admin-override-1".into(),
                    actor_id: Some(Uuid::now_v7()),
                },
            )
            .await
            .unwrap();
        assert!(!override_record.duplicate);
        let duplicate = operations
            .record_out_of_band_override(
                &reduced,
                OutOfBandOverrideRequest {
                    target_kind: "pipeline_run".into(),
                    target_id: Uuid::now_v7(),
                    action: "pause".into(),
                    reason: "retry of the same request".into(),
                    idempotency_key: "admin-override-1".into(),
                    actor_id: Some(Uuid::now_v7()),
                },
            )
            .await
            .unwrap();
        assert!(duplicate.duplicate);
        let reduced = operations
            .reduce_binding(reduced, "self-origin-reducer")
            .await
            .unwrap();
        let reductions = db.fetch_orchestration_reductions(binding.id).await.unwrap();
        let override_reduction = reductions
            .iter()
            .find(|reduction| {
                reduction
                    .detail
                    .pointer("/event/event_type")
                    .and_then(Value::as_str)
                    == Some("out_of_band_override")
            })
            .expect("administrator override should pass through the reducer timeline");
        assert_eq!(override_reduction.disposition, "observed");
        assert_eq!(
            override_reduction
                .detail
                .pointer("/event/payload/action")
                .and_then(Value::as_str),
            Some("pause")
        );

        db.record_ingress_event(
            binding.admission_id,
            binding.generation,
            IngressEvent {
                source: "runinator.manual".into(),
                event_id: "manual-stop".into(),
                event_type: "manual_intent".into(),
                correlation_key: binding.correlation_key.clone(),
                payload: runinator_models::json!({
                    "intent": "stop",
                    "payload": { "reason": "operator request" },
                    "reason": "audit reason",
                }),
                provenance: Value::Null,
                occurred_at: Some(now),
            },
            IngressEventDisposition::Recorded,
            false,
            now,
        )
        .await
        .unwrap();
        let reduced = OrchestrationOperations::new(db.clone())
            .reduce_binding(reduced, "self-origin-reducer")
            .await
            .unwrap();
        assert_eq!(reduced.status, OrchestrationStatus::Terminated);
        assert!(
            db.fetch_orchestration_pending_intents(binding.id)
                .await
                .unwrap()
                .is_empty()
        );
        let commands = db.fetch_orchestration_commands(binding.id).await.unwrap();
        assert_eq!(
            commands
                .iter()
                .find(|command| command.command_type == "cancel_epoch")
                .map(|command| command.payload.clone()),
            Some(runinator_models::json!({ "reason": "operator request" }))
        );
        let reductions = db.fetch_orchestration_reductions(binding.id).await.unwrap();
        assert_eq!(reductions.last().unwrap().disposition, "applied");

        drop(db);
        let _ = std::fs::remove_file(path);
    }
}

mod intent_decision;
pub use intent_decision::IntentDecision;

mod orchestration_operations;
pub use orchestration_operations::OrchestrationOperations;

mod out_of_band_override_request;
pub use out_of_band_override_request::OutOfBandOverrideRequest;
