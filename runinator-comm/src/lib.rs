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
    server_settings::WakerSettings,
    value::Value,
    workflow_vm::{
        UnsupportedWorkflowVmVersion, WORKFLOW_EFFECT_PROTOCOL_VERSION, WorkflowEffectRequest,
        WorkflowEffectStatus, ensure_effect_protocol_version,
    },
};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

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

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EffectExecutor {
    Provider,
    Infrastructure,
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
    Progress {
        kind: String,
        #[serde(default)]
        payload: Value,
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

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum UiEventKind {
    WorkflowsChanged,
    ExecutionProfilesChanged,
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
    IngressControlChanged {
        stream: String,
        record_id: Uuid,
        state: String,
        owner_scope: runinator_models::rbac::ScopeRef,
    },
}

#[cfg(test)]
mod tests;

mod worker_peer;
pub use worker_peer::WorkerPeer;

mod worker_announcement;
pub use worker_announcement::WorkerAnnouncement;

mod web_service_announcement;
pub use web_service_announcement::WebServiceAnnouncement;

mod effect_command;
pub use effect_command::EffectCommand;

mod effect_dispatch_record;
pub use effect_dispatch_record::EffectDispatchRecord;

mod notification_effect_dispatch_record;
pub use notification_effect_dispatch_record::NotificationEffectDispatchRecord;

mod effect_result;
pub use effect_result::EffectResult;

mod control_command;
pub use control_command::ControlCommand;

mod agent_command;
pub use agent_command::AgentCommand;

mod agent_directive_result;
pub use agent_directive_result::AgentDirectiveResult;

mod agent_directive_record;
pub use agent_directive_record::AgentDirectiveRecord;

mod wake_command;
pub use wake_command::WakeCommand;

mod timer_interrupt_wake;
pub use timer_interrupt_wake::TimerInterruptWake;

mod orchestration_intent_wake;
pub use orchestration_intent_wake::OrchestrationIntentWake;

mod debug_command;
pub use debug_command::DebugCommand;

mod ui_event;
pub use ui_event::UiEvent;
