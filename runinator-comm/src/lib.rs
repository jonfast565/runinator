pub mod discovery;
pub mod errors;
pub mod targeting;
pub mod wire;

pub use targeting::{ActionTarget, ConsumerProfile};
pub use wire::{WireCodec, WireError};

use chrono::{DateTime, Utc};
use runinator_models::{
    providers::ProviderMetadata,
    replicas::ReplicaRegistrationRequest,
    runs::{ProviderTerminalControl, TerminalInteraction},
    value::Value,
    workflow_vm::{
        UnsupportedWorkflowVmVersion, WORKFLOW_EFFECT_PROTOCOL_VERSION, WorkflowEffectRequest,
        WorkflowEffectStatus, ensure_effect_protocol_version,
    },
};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkerPeer {
    pub worker_id: Uuid,
    pub address: String,
    pub last_heartbeat: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkerAnnouncement {
    pub worker_id: Uuid,
    pub address: String,
    pub last_heartbeat: DateTime<Utc>,
    pub known_peers: Vec<WorkerPeer>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WebServiceAnnouncement {
    pub service_id: Uuid,
    pub address: String,
    pub port: u16,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub base_path: Option<String>,
    #[serde(default = "default_service_scheme")]
    pub scheme: String,
    #[serde(default = "default_relay_path")]
    pub relay_path: String,
    #[serde(default)]
    pub cluster_id: Uuid,
    #[serde(default)]
    pub enrollment_enabled: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub spki_pin: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub version: Option<String>,
    pub last_heartbeat: DateTime<Utc>,
}

fn default_service_scheme() -> String {
    "http".to_string()
}

fn default_relay_path() -> String {
    "/ws/broker".to_string()
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum GossipMessage {
    Worker { worker: WorkerAnnouncement },
    WebService { service: WebServiceAnnouncement },
}

/// Generic durable work published by the workflow VM host.
///
/// This is not coupled to a node-run record. The effect id identifies
/// the one persisted receipt that a result may settle, and the continuation id identifies exactly
/// which suspended VM branch becomes runnable afterwards.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EffectCommand {
    pub version: u32,
    pub command_id: Uuid,
    pub effect_id: Uuid,
    pub workflow_run_id: Uuid,
    pub continuation_id: Uuid,
    pub attempt: u32,
    pub request: WorkflowEffectRequest,
    /// Selects the class of host allowed to claim this command. Provider workers and the
    /// infrastructure coordinator share the effect protocol, but must never compete for the same
    /// request kind.
    pub executor: EffectExecutor,
    #[serde(default)]
    pub target: ActionTarget,
    #[serde(default = "Uuid::now_v7")]
    pub trace_id: Uuid,
    #[serde(default, skip_serializing_if = "std::collections::HashMap::is_empty")]
    pub trace_context: std::collections::HashMap<String, String>,
    pub idempotency_key: String,
    /// Set for an engine-owned notification delivery. Such a command shares the provider-effect
    /// transport and worker executor, but is settled against `notification_deliveries`, never a
    /// workflow effect receipt or continuation.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub notification_delivery_id: Option<Uuid>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EffectExecutor {
    Provider,
    Infrastructure,
}

/// One leased entry in the VM effect-delivery outbox.
///
/// This deliberately carries the complete frozen command: a publisher must never reconstruct an
/// effect from mutable workflow state after the receipt was committed.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EffectDispatchRecord {
    pub id: Uuid,
    pub effect_id: Uuid,
    pub dedupe_key: String,
    pub command: EffectCommand,
    pub attempts: i64,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub published_at: Option<DateTime<Utc>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_error: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub claimed_by: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub claimed_until: Option<DateTime<Utc>>,
}

/// One leased external notification delivery. It deliberately uses the provider-effect envelope so
/// workers share the same provider runtime, while its receipt and settlement remain outside the
/// workflow VM's continuation/effect tables.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NotificationEffectDispatchRecord {
    pub delivery_id: Uuid,
    pub dedupe_key: String,
    pub command: EffectCommand,
    pub attempts: i64,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub published_at: Option<DateTime<Utc>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_error: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub claimed_by: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub claimed_until: Option<DateTime<Utc>>,
}

impl EffectCommand {
    pub fn is_supported(&self) -> bool {
        self.version == WORKFLOW_EFFECT_PROTOCOL_VERSION
    }

    pub fn ensure_supported(&self) -> Result<(), UnsupportedWorkflowVmVersion> {
        ensure_effect_protocol_version(self.version)
    }
}

/// A worker or infrastructure host's terminal or streaming report for one VM effect.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EffectResult {
    pub version: u32,
    pub event_id: Uuid,
    pub effect_id: Uuid,
    pub workflow_run_id: Uuid,
    pub continuation_id: Uuid,
    pub attempt: u32,
    pub kind: EffectResultKind,
    pub timestamp: DateTime<Utc>,
    #[serde(default = "Uuid::now_v7")]
    pub trace_id: Uuid,
    /// Copied from the originating command when this result settles a durable notification
    /// delivery instead of a workflow-owned effect receipt.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub notification_delivery_id: Option<Uuid>,
}

impl EffectResult {
    pub fn is_supported(&self) -> bool {
        self.version == WORKFLOW_EFFECT_PROTOCOL_VERSION
    }

    pub fn ensure_supported(&self) -> Result<(), UnsupportedWorkflowVmVersion> {
        ensure_effect_protocol_version(self.version)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum EffectResultKind {
    Status {
        status: WorkflowEffectStatus,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        output: Option<Value>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        message: Option<String>,
    },
    Chunk {
        stream: String,
        content: String,
    },
    Artifact {
        artifact: Value,
    },
    TerminalInteraction {
        interaction: TerminalInteraction,
    },
    /// The executing host has taken this attempt. It carries the executor's replica id, which is
    /// the VM's executor lease — the fact replica load and stale-replica reaping read now that
    /// node runs are gone. It is advisory: an effect settles whether or not a claim arrived.
    Claimed {
        executor_replica_id: Uuid,
    },
}

impl EffectResult {
    pub fn status(
        command: &EffectCommand,
        status: WorkflowEffectStatus,
        output: Option<Value>,
        message: Option<String>,
    ) -> Self {
        Self {
            version: WORKFLOW_EFFECT_PROTOCOL_VERSION,
            event_id: Uuid::now_v7(),
            effect_id: command.effect_id,
            workflow_run_id: command.workflow_run_id,
            continuation_id: command.continuation_id,
            attempt: command.attempt,
            kind: EffectResultKind::Status {
                status,
                output,
                message,
            },
            timestamp: Utc::now(),
            trace_id: command.trace_id,
            notification_delivery_id: command.notification_delivery_id,
        }
    }

    /// Announce that `executor_replica_id` has taken this attempt.
    pub fn claimed(command: &EffectCommand, executor_replica_id: Uuid) -> Self {
        Self {
            version: WORKFLOW_EFFECT_PROTOCOL_VERSION,
            event_id: Uuid::now_v7(),
            effect_id: command.effect_id,
            workflow_run_id: command.workflow_run_id,
            continuation_id: command.continuation_id,
            attempt: command.attempt,
            kind: EffectResultKind::Claimed {
                executor_replica_id,
            },
            timestamp: Utc::now(),
            trace_id: command.trace_id,
            notification_delivery_id: command.notification_delivery_id,
        }
    }
}

#[cfg(test)]
mod effect_protocol_tests {
    use super::*;
    use runinator_models::workflow_vm::WorkflowEffectRequest;

    #[test]
    fn status_result_preserves_effect_and_continuation_correlation() {
        let command = EffectCommand {
            version: WORKFLOW_EFFECT_PROTOCOL_VERSION,
            command_id: Uuid::now_v7(),
            effect_id: Uuid::now_v7(),
            workflow_run_id: Uuid::now_v7(),
            continuation_id: Uuid::now_v7(),
            attempt: 3,
            request: WorkflowEffectRequest::Timer { due_at: 1 },
            executor: EffectExecutor::Infrastructure,
            target: ActionTarget::Any,
            trace_id: Uuid::now_v7(),
            trace_context: std::collections::HashMap::new(),
            idempotency_key: "effect-key".into(),
            notification_delivery_id: None,
        };
        let result = EffectResult::status(
            &command,
            WorkflowEffectStatus::Succeeded,
            Some(Value::String("ok".into())),
            None,
        );
        assert_eq!(result.effect_id, command.effect_id);
        assert_eq!(result.continuation_id, command.continuation_id);
        assert_eq!(result.attempt, command.attempt);
        assert!(command.is_supported());
        assert!(result.is_supported());
    }

    #[test]
    fn incompatible_effect_protocol_is_rejected_before_handling() {
        let raw = format!(
            r#"{{"version":{},"command_id":"00000000-0000-0000-0000-000000000000","effect_id":"00000000-0000-0000-0000-000000000000","workflow_run_id":"00000000-0000-0000-0000-000000000000","continuation_id":"00000000-0000-0000-0000-000000000000","attempt":0,"request":{{"type":"timer","due_at":1}},"executor":"infrastructure","idempotency_key":"x"}}"#,
            WORKFLOW_EFFECT_PROTOCOL_VERSION + 1
        );
        let command: EffectCommand = serde_json::from_str(&raw).unwrap();
        assert!(!command.is_supported());
        assert_eq!(
            command.ensure_supported().unwrap_err().actual,
            WORKFLOW_EFFECT_PROTOCOL_VERSION + 1
        );
    }

    #[test]
    fn effect_command_has_a_pinned_json_shape() {
        let command = EffectCommand {
            version: WORKFLOW_EFFECT_PROTOCOL_VERSION,
            command_id: Uuid::nil(),
            effect_id: Uuid::nil(),
            workflow_run_id: Uuid::nil(),
            continuation_id: Uuid::nil(),
            attempt: 0,
            request: WorkflowEffectRequest::Timer { due_at: 1 },
            executor: EffectExecutor::Infrastructure,
            target: ActionTarget::Any,
            trace_id: Uuid::nil(),
            trace_context: std::collections::HashMap::new(),
            idempotency_key: "key".into(),
            notification_delivery_id: None,
        };
        assert_eq!(
            serde_json::to_string(&command).unwrap(),
            r#"{"version":1,"command_id":"00000000-0000-0000-0000-000000000000","effect_id":"00000000-0000-0000-0000-000000000000","workflow_run_id":"00000000-0000-0000-0000-000000000000","continuation_id":"00000000-0000-0000-0000-000000000000","attempt":0,"request":{"type":"timer","due_at":1},"executor":"infrastructure","target":{"kind":"any"},"trace_id":"00000000-0000-0000-0000-000000000000","idempotency_key":"key"}"#
        );
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ControlKind {
    Cancel,
    Pause,
    Resume,
    /// Deliver input or geometry to a provider-owned PTY without changing workflow state.
    Terminal,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ControlCommand {
    pub workflow_run_id: Uuid,
    pub kind: ControlKind,
    /// when set, the control applies to a single node run rather than the whole run. used to cancel
    /// an already-dispatched losing race branch without disturbing the winner or sibling work.
    /// defaults to `None` for backward-compatible deserialization of run-wide commands.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub workflow_node_run_id: Option<Uuid>,
    /// VM execution target. Mutually exclusive with `workflow_node_run_id`; when present the
    /// control reaches exactly the provider effect identified here.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub effect_id: Option<Uuid>,
    /// runtime routing key selecting which worker(s) should receive this control. the web service
    /// stamps the executing worker's replica (from the node run's executor claim) on cancels so
    /// they reach the holder instead of a random control consumer; `Any` (the default, and the
    /// deserialization of older messages) preserves the untargeted competing-consumer behavior.
    #[serde(default)]
    pub target: ActionTarget,
    /// Present only when `kind` is `terminal`. Kept separate from `kind` so the established
    /// cancel/pause/resume representation remains backward compatible.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub terminal: Option<ProviderTerminalControl>,
}

/// replica-scoped fleet-management command. unlike [`ControlCommand`], this is never associated
/// with a workflow run.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentCommand {
    pub directive_id: Uuid,
    pub replica_id: Uuid,
    pub target: ActionTarget,
    pub kind: AgentDirectiveKind,
    pub issued_at: DateTime<Utc>,
    pub expires_at: DateTime<Utc>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum AgentDirectiveKind {
    Diagnostics,
    TailLogs {
        lines: usize,
    },
    ListSandbox {
        path: String,
    },
    FetchFile {
        path: String,
        max_bytes: u64,
    },
    /// Idempotently remove one opaque workspace key beneath the worker's configured workspace
    /// root. The control plane never receives or constructs a host filesystem path.
    CleanupWorkspace {
        workspace_id: Uuid,
        local_key: String,
    },
    SetLabels {
        labels: std::collections::BTreeMap<String, String>,
    },
    SetConcurrency {
        max_concurrent_actions: usize,
    },
    SetLogLevel {
        level: String,
    },
    RepublishProviders,
    Drain,
    Undrain,
    Restart,
    RotateCredential,
    /// forward-compatible catch-all: older agents can report unsupported instead of rejecting the
    /// entire command envelope during deserialization.
    #[serde(other)]
    Unknown,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AgentDirectiveStatus {
    Accepted,
    Completed,
    Failed,
    Unsupported,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentDirectiveResult {
    pub directive_id: Uuid,
    pub status: AgentDirectiveStatus,
    #[serde(default)]
    pub payload: Value,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
}

/// durable server-side lifecycle for one replica directive.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AgentDirectiveState {
    Pending,
    Published,
    Accepted,
    Completed,
    Failed,
    Unsupported,
    Expired,
}

/// persisted directive intent and its eventual agent reply.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentDirectiveRecord {
    pub directive_id: Uuid,
    pub replica_id: Uuid,
    pub kind: AgentDirectiveKind,
    pub state: AgentDirectiveState,
    pub issued_at: DateTime<Utc>,
    pub expires_at: DateTime<Utc>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub published_at: Option<DateTime<Utc>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub completed_at: Option<DateTime<Utc>>,
    #[serde(default)]
    pub payload: Value,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
    pub attempts: i64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub claimed_at: Option<DateTime<Utc>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub claimed_by_runtime_id: Option<String>,
}

/// a timer ticket for a workflow VM effect that completes at a known instant.
///
/// the infrastructure effect host publishes one of these instead of sleeping in-process, carrying
/// the terminal [`EffectResult`] it would have returned; the waker is the sole consumer and relays
/// a [`WsIngressCommand::SettleEffect`] once due. the result is carried rather than rebuilt so the
/// waker needs no database and the settle path stays the ordinary effect-result path.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WakeCommand {
    /// the instant this wake becomes due and its result should be handed back.
    pub due_at: DateTime<Utc>,
    /// the effect result to publish once due. its `timestamp` is already stamped at `due_at`, so a
    /// late relay never backdates or forward-dates the settlement.
    pub result: EffectResult,
    /// correlation id minted when this wake is published, carried through the waker into the
    /// resulting [`WsIngressCommand::SettleEffect`] so a stuck or delayed wake can be traced end to
    /// end. defaults for backward-compatible deserialization of older messages.
    #[serde(default = "Uuid::now_v7")]
    pub trace_id: Uuid,
    /// A workflow-level periodic timer interrupt. When present, the waker relays an ingress
    /// command instead of settling `result`; `result` remains populated for wire compatibility
    /// with effect wakes and is intentionally ignored by the timer-interrupt path.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub timer_interrupt: Option<TimerInterruptWake>,
    /// A provider-neutral nudge for a durable coalescing window. The pending-intent row remains
    /// authoritative; the waker merely tells an engine replica that its deadline has arrived.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub orchestration_intent: Option<OrchestrationIntentWake>,
}

/// A due occurrence of one configured workflow timer. The timer id names a frozen interrupt
/// declaration, so several independent periods can coexist on the same run.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TimerInterruptWake {
    pub workflow_run_id: Uuid,
    pub timer_id: String,
    pub interval_seconds: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OrchestrationIntentWake {
    pub binding_id: Uuid,
    pub intent: String,
}

impl WakeCommand {
    pub fn new(due_at: DateTime<Utc>, result: EffectResult, trace_id: Uuid) -> Self {
        Self {
            due_at,
            result,
            trace_id,
            timer_interrupt: None,
            orchestration_intent: None,
        }
    }

    /// Build a wake for a workflow-owned periodic timer. `result` is a compatibility marker only:
    /// the waker detects `timer_interrupt` and never sends it to the effect-settlement path.
    pub fn timer_interrupt(
        due_at: DateTime<Utc>,
        workflow_run_id: Uuid,
        timer_id: impl Into<String>,
        interval_seconds: i64,
        trace_id: Uuid,
    ) -> Self {
        Self {
            due_at,
            result: EffectResult {
                version: WORKFLOW_EFFECT_PROTOCOL_VERSION,
                event_id: Uuid::now_v7(),
                effect_id: Uuid::now_v7(),
                workflow_run_id,
                continuation_id: Uuid::nil(),
                attempt: 0,
                kind: EffectResultKind::Status {
                    status: WorkflowEffectStatus::Succeeded,
                    output: None,
                    message: None,
                },
                timestamp: due_at,
                trace_id,
                notification_delivery_id: None,
            },
            trace_id,
            timer_interrupt: Some(TimerInterruptWake {
                workflow_run_id,
                timer_id: timer_id.into(),
                interval_seconds,
            }),
            orchestration_intent: None,
        }
    }

    /// Build an opaque coalescing-deadline wake. The compatibility result is never settled: the
    /// waker recognizes `orchestration_intent` and relays the typed ingress nudge instead.
    pub fn orchestration_intent(
        due_at: DateTime<Utc>,
        binding_id: Uuid,
        intent: impl Into<String>,
        trace_id: Uuid,
    ) -> Self {
        Self {
            due_at,
            result: EffectResult {
                version: WORKFLOW_EFFECT_PROTOCOL_VERSION,
                event_id: Uuid::now_v7(),
                effect_id: Uuid::nil(),
                workflow_run_id: Uuid::nil(),
                continuation_id: Uuid::nil(),
                attempt: 0,
                kind: EffectResultKind::Status {
                    status: WorkflowEffectStatus::Succeeded,
                    output: None,
                    message: None,
                },
                timestamp: due_at,
                trace_id,
                notification_delivery_id: None,
            },
            trace_id,
            timer_interrupt: None,
            orchestration_intent: Some(OrchestrationIntentWake {
                binding_id,
                intent: intent.into(),
            }),
        }
    }

    /// the effect this wake settles.
    pub fn effect_id(&self) -> Uuid {
        self.result.effect_id
    }

    /// the run this wake settles an effect for.
    pub fn workflow_run_id(&self) -> Uuid {
        self.result.workflow_run_id
    }

    /// stable identity for broker deduplication while a wake is in flight. keyed on the attempt so
    /// a retried effect arms a new timer rather than colliding with the one it replaced.
    pub fn dedupe_key(&self) -> String {
        if let Some(intent) = &self.orchestration_intent {
            return format!(
                "orchestration-intent:{}:{}:{}",
                intent.binding_id,
                intent.intent,
                self.due_at.timestamp_millis()
            );
        }
        if let Some(timer) = &self.timer_interrupt {
            return format!(
                "timer-interrupt:{}:{}:{}",
                timer.workflow_run_id,
                timer.timer_id,
                self.due_at.timestamp()
            );
        }
        format!("{}:{}", self.result.effect_id, self.result.attempt)
    }
}

/// One availability observation sent by a broker-only runtime to the engine.
///
/// `Available` is both registration and heartbeat: the runtime owns its identity before the
/// asynchronous message is applied, which lets a worker safely use that identity in broker targets
/// and effect claims. The engine is the single durable writer of the replica row. A clean shutdown
/// sends `Offline`; missed observations still fall through to the normal stale/reap policy.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "state", rename_all = "snake_case")]
pub enum ReplicaAvailability {
    Available {
        registration: ReplicaRegistrationRequest,
        /// Provider declarations travel with availability so a broker-only worker does not need a
        /// separate web-service API call just to be discoverable.
        #[serde(default)]
        providers: Vec<ProviderMetadata>,
    },
    Offline {
        replica_id: Uuid,
        runtime_id: String,
    },
}

/// A message addressed to the engine from a non-web-service runtime, carried on the ingress
/// channel. The engine is the sole consumer, so producers never depend on each other.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum WsIngressCommand {
    /// waker -> engine: a timer wake came due; settle its effect with the carried result.
    SettleEffect {
        result: EffectResult,
        /// carried over from the originating [`WakeCommand::trace_id`]. defaults for
        /// backward-compatible deserialization of older messages.
        #[serde(default = "Uuid::now_v7")]
        trace_id: Uuid,
    },
    /// waker -> engine: a workflow-owned periodic timer elapsed. The engine records the pending
    /// interrupt and advances this declaration's durable schedule atomically.
    TimerInterrupt {
        timer: TimerInterruptWake,
        due_at: DateTime<Utc>,
        #[serde(default = "Uuid::now_v7")]
        trace_id: Uuid,
    },
    /// waker -> engine: a durable coalescing deadline arrived. The reducer reloads pending state
    /// and decides whether this wake is current, superseded, or already consumed.
    OrchestrationIntent {
        wake: OrchestrationIntentWake,
        due_at: DateTime<Utc>,
        #[serde(default = "Uuid::now_v7")]
        trace_id: Uuid,
    },
    /// worker -> WS: a control request from an executing action.
    Control {
        workflow_run_id: Uuid,
        kind: ControlKind,
    },
    /// agent -> WS: completion or refusal of a durable fleet command.
    AgentDirectiveResult { result: AgentDirectiveResult },
    /// non-web-service runtime -> engine: durable lifecycle observation. This is intentionally
    /// broker mediated so data-plane runtimes do not need to call the web service to appear in the
    /// fleet.
    ReplicaAvailability { availability: ReplicaAvailability },
}

impl WsIngressCommand {
    pub fn settle_effect(result: EffectResult, trace_id: Uuid) -> Self {
        Self::SettleEffect { result, trace_id }
    }

    pub fn timer_interrupt(
        timer: TimerInterruptWake,
        due_at: DateTime<Utc>,
        trace_id: Uuid,
    ) -> Self {
        Self::TimerInterrupt {
            timer,
            due_at,
            trace_id,
        }
    }

    pub fn orchestration_intent(
        wake: OrchestrationIntentWake,
        due_at: DateTime<Utc>,
        trace_id: Uuid,
    ) -> Self {
        Self::OrchestrationIntent {
            wake,
            due_at,
            trace_id,
        }
    }

    pub fn control(workflow_run_id: Uuid, kind: ControlKind) -> Self {
        Self::Control {
            workflow_run_id,
            kind,
        }
    }

    pub fn replica_available(
        registration: ReplicaRegistrationRequest,
        providers: Vec<ProviderMetadata>,
    ) -> Self {
        Self::ReplicaAvailability {
            availability: ReplicaAvailability::Available {
                registration,
                providers,
            },
        }
    }

    pub fn replica_offline(replica_id: Uuid, runtime_id: impl Into<String>) -> Self {
        Self::ReplicaAvailability {
            availability: ReplicaAvailability::Offline {
                replica_id,
                runtime_id: runtime_id.into(),
            },
        }
    }

    /// stable identity for broker deduplication while a message is in flight.
    pub fn dedupe_key(&self) -> String {
        match self {
            Self::SettleEffect { result, .. } => {
                format!("settle:{}:{}", result.effect_id, result.attempt)
            }
            Self::TimerInterrupt { timer, due_at, .. } => format!(
                "timer-interrupt:{}:{}:{}",
                timer.workflow_run_id,
                timer.timer_id,
                due_at.timestamp()
            ),
            Self::OrchestrationIntent { wake, due_at, .. } => format!(
                "orchestration-intent:{}:{}:{}",
                wake.binding_id,
                wake.intent,
                due_at.timestamp_millis()
            ),
            Self::Control {
                workflow_run_id,
                kind,
            } => format!("control:{workflow_run_id}:{kind:?}"),
            Self::AgentDirectiveResult { result } => {
                format!(
                    "agent-directive-result:{}:{:?}",
                    result.directive_id, result.status
                )
            }
            Self::ReplicaAvailability { availability } => match availability {
                ReplicaAvailability::Available { registration, .. } => {
                    format!(
                        "replica-available:{}:{}",
                        registration.replica_id.unwrap_or_default(),
                        registration.runtime_id
                    )
                }
                ReplicaAvailability::Offline {
                    replica_id,
                    runtime_id,
                } => format!("replica-offline:{replica_id}:{runtime_id}"),
            },
        }
    }
}

/// The canonical VM debugger operations against a run. One tagged contract keeps the frontend,
/// web service, and future broker paths aligned.
/// which thread of control a debug verb addresses.
///
/// `None` means "the one the operator is looking at" — the first parked cursor, else the primary.
/// every verb that acts on a position carries this, because a run with fan-out has several and
/// stepping the wrong one is not a recoverable mistake. omitting it keeps single-cursor clients
/// working unchanged: the field is absent from the wire in that case.
pub type CursorTarget = Option<Uuid>;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "verb", rename_all = "snake_case")]
pub enum DebugVerb {
    /// Advance exactly one VM boundary, then pause again.
    Step {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        cursor: CursorTarget,
    },
    /// resume normal execution.
    Continue {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        cursor: CursorTarget,
    },
    /// Resume one parked branch until it reaches `node_id` or an ordinary breakpoint first.
    RunTo { cursor: Uuid, node_id: String },
    /// Replace the run-scoped breakpoint set used by every continuation.
    SetBreakpoints { breakpoints: Vec<String> },
    /// Pause each branch once, before its next failure is routed.
    SetPauseOnFailure { enabled: bool },
}

impl runinator_models::validation::Validate for DebugVerb {
    fn validate(&self) -> Result<(), runinator_models::validation::ValidationError> {
        Ok(())
    }
}

/// a [`DebugVerb`] addressed to a specific workflow run.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DebugCommand {
    pub workflow_run_id: Uuid,
    #[serde(flatten)]
    pub verb: DebugVerb,
}

impl DebugCommand {
    pub fn new(workflow_run_id: Uuid, verb: DebugVerb) -> Self {
        Self {
            workflow_run_id,
            verb,
        }
    }
}

/// A live UI hint sent to every web-service replica so connected WebSocket clients can refetch.
/// Delivery is best effort. A dropped event only leaves a panel briefly stale.
/// Each replica can drop the event at WebSocket egress when [`Self::org_id`] does not match the
/// caller's active organization.
///
/// wire shape keeps the historical tagged `type` field via flatten, with an optional sibling
/// `org_id`. older publishers that omit `org_id` deserialize as unscoped (`None`) and remain
/// visible to every client during the rollout phase.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UiEvent {
    /// When set, WS egress delivers only to platform admins and clients in the active organization.
    /// When absent, every connected client can see the event.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub org_id: Option<Uuid>,
    #[serde(flatten)]
    pub kind: UiEventKind,
}

impl UiEvent {
    pub fn new(org_id: Option<Uuid>, kind: UiEventKind) -> Self {
        Self { org_id, kind }
    }

    /// unscoped / platform-global hint.
    pub fn global(kind: UiEventKind) -> Self {
        Self::new(None, kind)
    }

    pub fn for_org(org_id: Uuid, kind: UiEventKind) -> Self {
        Self::new(Some(org_id), kind)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum UiEventKind {
    WorkflowsChanged,
    WorkflowRunChanged {
        run_id: Uuid,
    },
    WorkflowRunActivity,
    PipelineRunChanged {
        run_id: Uuid,
    },
    PipelineRunActivity,
    OrchestrationChanged {
        orchestration_id: Uuid,
    },
    AdapterChanged {
        adapter_id: Uuid,
    },
    ExternalOperationChanged {
        operation_id: Uuid,
        orchestration_id: Uuid,
    },
    NotificationCreated {
        notification_id: Uuid,
    },
    NotificationsChanged,
    ReplicasChanged,
    /// a freeze window was created, edited, or removed, so what is currently suspended changed.
    SchedulesChanged,
}

impl ControlCommand {
    pub fn new(workflow_run_id: Uuid, kind: ControlKind) -> Self {
        Self {
            workflow_run_id,
            kind,
            workflow_node_run_id: None,
            effect_id: None,
            target: ActionTarget::Any,
            terminal: None,
        }
    }

    /// a control targeting a single node run (e.g. cancelling one losing race branch).
    pub fn for_node_run(
        workflow_run_id: Uuid,
        workflow_node_run_id: Uuid,
        kind: ControlKind,
    ) -> Self {
        Self {
            workflow_run_id,
            kind,
            workflow_node_run_id: Some(workflow_node_run_id),
            effect_id: None,
            target: ActionTarget::Any,
            terminal: None,
        }
    }

    pub fn for_effect(workflow_run_id: Uuid, effect_id: Uuid, kind: ControlKind) -> Self {
        Self {
            workflow_run_id,
            kind,
            workflow_node_run_id: None,
            effect_id: Some(effect_id),
            target: ActionTarget::Any,
            terminal: None,
        }
    }

    pub fn for_terminal(
        workflow_run_id: Uuid,
        effect_id: Uuid,
        terminal: ProviderTerminalControl,
    ) -> Self {
        Self {
            workflow_run_id,
            kind: ControlKind::Terminal,
            workflow_node_run_id: None,
            effect_id: Some(effect_id),
            target: ActionTarget::Any,
            terminal: Some(terminal),
        }
    }

    /// route this control to the worker replica currently holding the executor lease, so it is not
    /// consumed (and dropped) by a worker that never dispatched the action.
    pub fn targeting_replica(mut self, replica_id: Uuid) -> Self {
        self.target = ActionTarget::Replica { replica_id };
        self
    }
}

#[cfg(test)]
mod tests;
