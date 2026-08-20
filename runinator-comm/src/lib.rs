pub mod discovery;
pub mod errors;
pub mod targeting;
pub mod wire;

pub use targeting::{ActionTarget, ConsumerProfile};
pub use wire::{WireCodec, WireError};

use chrono::{DateTime, Utc};
use runinator_models::{
    runs::{NewRunArtifact, NewRunChunk},
    value::Value,
    workflow_state::DebugMode,
    workflow_vm::{
        UnsupportedWorkflowVmVersion, WORKFLOW_EFFECT_PROTOCOL_VERSION, WorkflowEffectRequest,
        WorkflowEffectStatus, ensure_effect_protocol_version,
    },
    workflows::{WorkflowAction, WorkflowStatus},
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
    "/ws/desktop-worker".to_string()
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum GossipMessage {
    Worker { worker: WorkerAnnouncement },
    WebService { service: WebServiceAnnouncement },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ActionCommand {
    pub command_id: Uuid,
    pub workflow_run_id: Uuid,
    pub workflow_node_run_id: Uuid,
    pub node_id: String,
    pub action: WorkflowAction,
    pub attempt: i64,
    #[serde(default)]
    pub parameters: Value,
    /// runtime routing key selecting which worker(s) may receive this action. the reducer stamps it
    /// at dispatch; defaults to `Any` for backward-compatible deserialization of older messages.
    #[serde(default)]
    pub target: ActionTarget,
    /// correlation id propagated across the ws -> broker -> worker hop so spans/logs for one action
    /// execution line up. defaults for backward-compatible deserialization of older messages.
    #[serde(default = "Uuid::now_v7")]
    pub trace_id: Uuid,
    /// w3c trace context (e.g. `traceparent`) captured at dispatch so the worker's execution span
    /// joins the dispatching trace. empty when otel is off; defaults for older messages.
    #[serde(default, skip_serializing_if = "std::collections::HashMap::is_empty")]
    pub trace_context: std::collections::HashMap<String, String>,
    /// set when this action is a notification delivery rather than a workflow node's work. the
    /// engine reuses the action outbox so alert delivery runs through the normal provider path, and
    /// the result consumer settles this delivery row instead of a node run. `None` for node actions.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub notification_delivery_id: Option<Uuid>,
    /// set when this action is one durable call of a resumable invocation rather than a whole node.
    ///
    /// the second owner an action dispatch can have, following `notification_delivery_id`. an
    /// invocation makes N calls under one node run, so the node run id alone cannot say *which*
    /// call a result settles — this can, and the result consumer resumes the continuation parked on
    /// it instead of settling the node run. `None` for plain node actions.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub invocation_call_id: Option<Uuid>,
    /// set when this dispatch belongs to a durable RexRap `task[T]` handle. Its launcher node
    /// advances immediately; only the task record is settled when the worker returns.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub task_run_id: Option<Uuid>,
    /// resolved idempotency key for this action's external effect, from the node's
    /// `.idempotent(key: <expr>)`. the reducer evaluates the expression against the run context and
    /// stamps the result here; the worker reserves it before invoking the provider and replays a
    /// previously recorded result instead of executing again. `None` for non-idempotent actions,
    /// which is the default and the behaviour of every pre-existing message.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub idempotency_key: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ActionDispatchRecord {
    pub id: Uuid,
    pub dedupe_key: String,
    pub command: ActionCommand,
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

/// Generic durable work published by the workflow VM host.
///
/// Unlike [`ActionCommand`], this is not coupled to a node-run record. The effect id identifies
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

/// a timer ticket for a future-dated ready node. the web service publishes these when a ready
/// node's `ready_at` is still in the future (and the reconcile backstop re-publishes lost ones);
/// the waker is the sole consumer and relays a [`WsIngressCommand::Drive`] once due. already-due
/// ready nodes skip this channel: the web service publishes their Drive on ingress directly.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WakeCommand {
    pub ready_node_id: Uuid,
    pub workflow_run_id: Uuid,
    pub node_id: String,
    pub ready_at: DateTime<Utc>,
    pub source_event_id: Uuid,
    /// correlation id minted when this wake is published, carried through the waker into the
    /// resulting [`WsIngressCommand::Drive`] so a stuck or delayed wake can be traced end to end.
    /// defaults for backward-compatible deserialization of older messages.
    #[serde(default = "Uuid::now_v7")]
    pub trace_id: Uuid,
}

impl WakeCommand {
    pub fn new(
        ready_node_id: Uuid,
        workflow_run_id: Uuid,
        node_id: String,
        ready_at: DateTime<Utc>,
        source_event_id: Uuid,
        trace_id: Uuid,
    ) -> Self {
        Self {
            ready_node_id,
            workflow_run_id,
            node_id,
            ready_at,
            source_event_id,
            trace_id,
        }
    }

    /// stable identity for broker deduplication while a wake is in flight.
    pub fn dedupe_key(&self) -> String {
        format!("{}:{}", self.ready_node_id, self.source_event_id)
    }
}

/// a message addressed to the web service from a waker or a worker, carried on the ingress
/// channel. the web service is the sole consumer, so producers never depend on each other.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum WsIngressCommand {
    /// waker -> ws: run the reducer for a now-due ready node.
    Drive {
        ready_node_id: Uuid,
        workflow_run_id: Uuid,
        node_id: String,
        /// carried over from the originating [`WakeCommand::trace_id`]. defaults for
        /// backward-compatible deserialization of older messages.
        #[serde(default = "Uuid::now_v7")]
        trace_id: Uuid,
    },
    /// worker -> ws: a control request raised by an executing action.
    Control {
        workflow_run_id: Uuid,
        kind: ControlKind,
    },
    /// agent -> ws: completion or refusal of a durable fleet directive.
    AgentDirectiveResult { result: AgentDirectiveResult },
}

impl WsIngressCommand {
    pub fn drive(
        ready_node_id: Uuid,
        workflow_run_id: Uuid,
        node_id: String,
        trace_id: Uuid,
    ) -> Self {
        Self::Drive {
            ready_node_id,
            workflow_run_id,
            node_id,
            trace_id,
        }
    }

    pub fn control(workflow_run_id: Uuid, kind: ControlKind) -> Self {
        Self::Control {
            workflow_run_id,
            kind,
        }
    }

    /// stable identity for broker deduplication while a message is in flight.
    pub fn dedupe_key(&self) -> String {
        match self {
            Self::Drive { ready_node_id, .. } => format!("drive:{ready_node_id}"),
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
        }
    }
}

/// the canonical set of debugger operations against a run. one tagged contract replaces the prior
/// per-endpoint shapes so every layer (frontend, web service, future broker paths) names debug
/// operations identically. payload-carrying verbs (skip/rerun/set_*) live here rather than on the
/// unit-variant [`ControlKind`].
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
    /// advance exactly one node, then pause again.
    Step {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        cursor: CursorTarget,
    },
    /// resume normal execution (still honoring breakpoints).
    Continue {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        cursor: CursorTarget,
    },
    /// run until `node_id` is reached, pausing there once.
    RunToCursor {
        /// the node to stop at. named `cursor` on the wire before threads of control had ids; the
        /// alias keeps every existing client working.
        #[serde(alias = "cursor")]
        node_id: String,
        /// which thread of control to run; omit for the one the operator is looking at. renamed on
        /// the wire (not plain `cursor`) because `node_id`'s back-compat alias already claims that
        /// key — a plain `cursor` field here would silently never deserialize.
        #[serde(
            default,
            rename = "run_cursor",
            skip_serializing_if = "Option::is_none"
        )]
        cursor: CursorTarget,
    },
    /// mark the active node succeeded with a synthetic `output` and advance.
    Skip {
        #[serde(default)]
        output: Value,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        message: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        cursor: CursorTarget,
    },
    /// supersede the active node's latest attempt and re-execute it with `parameters`.
    Rerun {
        #[serde(default)]
        parameters: Value,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        cursor: CursorTarget,
    },
    /// replace the configured breakpoint set.
    ///
    /// run-scoped on purpose: a breakpoint is a property of the graph as authored, so every thread
    /// of control honors the same set. only the one-shot stop is per-cursor.
    SetBreakpoints { breakpoints: Vec<String> },
    /// set the step granularity.
    SetMode { mode: DebugMode },
    /// fork a speculative "what if" branch that walks beside the real ones.
    Fork {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        from_cursor: CursorTarget,
        /// where the fork enters; defaults to wherever its parent is standing.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        at_node: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        label: Option<String>,
        /// merge-patch overlaid on the fork's context, for "what if this value were different".
        #[serde(default)]
        context_patch: Value,
    },
    /// abandon a speculative branch.
    RetireCursor { cursor: Uuid },
    /// let a speculative cursor dispatch one node for real instead of shadowing it.
    ArmForReal {
        cursor: Uuid,
        node_id: String,
        #[serde(default)]
        armed: bool,
    },
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

/// a live UI hint fanned out to every web-service replica so connected WebSocket clients refetch.
/// best-effort: a dropped event at worst leaves a panel briefly stale until the next event. carried
/// on the broker fan-out `events` channel (every ws pod receives every event); each replica may then
/// drop events at WebSocket egress when [`Self::org_id`] does not match the caller's active org.
///
/// wire shape keeps the historical tagged `type` field via flatten, with an optional sibling
/// `org_id`. older publishers that omit `org_id` deserialize as unscoped (`None`) and remain
/// visible to every client during the rollout phase.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UiEvent {
    /// when set, ws egress delivers only to platform admins and clients whose active org matches.
    /// when absent, the event is treated as global (visible to every connected client).
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
    RunStatusChanged {
        run_id: Uuid,
        terminal: bool,
    },
    RunChunkAdded {
        run_id: Uuid,
    },
    WorkflowsChanged,
    WorkflowRunChanged {
        run_id: Uuid,
    },
    WorkflowRunActivity,
    PipelineRunChanged {
        run_id: Uuid,
    },
    PipelineRunActivity,
    TasksChanged,
    ArtifactCreated {
        artifact_id: Uuid,
        run_id: Uuid,
    },
    NotificationCreated {
        notification_id: Uuid,
    },
    NotificationsChanged,
    ReplicasChanged,
    /// a freeze window was created, edited, or removed, so what is currently suspended changed.
    SchedulesChanged,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkflowResultEvent {
    pub event_id: Uuid,
    pub command_id: Uuid,
    pub workflow_run_id: Uuid,
    pub workflow_node_run_id: Uuid,
    pub node_id: String,
    /// the dispatch attempt (from the originating [`ActionCommand`]) this result belongs to, so a
    /// very late result from a superseded attempt cannot overwrite a retry's status. defaults to 0
    /// (unknown) for backward-compatible deserialization of older messages, which are applied
    /// unconditionally as before.
    #[serde(default)]
    pub attempt: i64,
    pub kind: WorkflowResultEventKind,
    pub timestamp: DateTime<Utc>,
    /// correlation id carried back from the originating [`ActionCommand`] so worker result handling
    /// stays on the same trace. defaults for backward-compatible deserialization of older messages.
    #[serde(default = "Uuid::now_v7")]
    pub trace_id: Uuid,
    /// carried back from the originating [`ActionCommand`]; when set, this result settles a
    /// notification delivery rather than a workflow node run.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub notification_delivery_id: Option<Uuid>,
    /// carried back from the originating [`ActionCommand`]; when set, this result settles one
    /// durable call of a resumable invocation rather than the node run as a whole.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub invocation_call_id: Option<Uuid>,
    /// copied from the originating action command. A task result settles its independent task
    /// record rather than the already-completed launch node.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub task_run_id: Option<Uuid>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum WorkflowResultEventKind {
    Status {
        status: WorkflowStatus,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        output_json: Option<Value>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        message: Option<String>,
    },
    Chunk {
        chunk: NewRunChunk,
    },
    Artifact {
        artifact: NewRunArtifact,
    },
}

impl ControlCommand {
    pub fn new(workflow_run_id: Uuid, kind: ControlKind) -> Self {
        Self {
            workflow_run_id,
            kind,
            workflow_node_run_id: None,
            effect_id: None,
            target: ActionTarget::Any,
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
        }
    }

    pub fn for_effect(workflow_run_id: Uuid, effect_id: Uuid, kind: ControlKind) -> Self {
        Self {
            workflow_run_id,
            kind,
            workflow_node_run_id: None,
            effect_id: Some(effect_id),
            target: ActionTarget::Any,
        }
    }

    /// route this control to the worker replica currently holding the executor lease, so it is not
    /// consumed (and dropped) by a worker that never dispatched the action.
    pub fn targeting_replica(mut self, replica_id: Uuid) -> Self {
        self.target = ActionTarget::Replica { replica_id };
        self
    }
}

impl WorkflowResultEvent {
    pub fn status(
        command: &ActionCommand,
        status: WorkflowStatus,
        output_json: Option<Value>,
        message: Option<String>,
    ) -> Self {
        Self::new(
            command,
            WorkflowResultEventKind::Status {
                status,
                output_json,
                message,
            },
        )
    }

    pub fn chunk(command: &ActionCommand, chunk: NewRunChunk) -> Self {
        Self::new(command, WorkflowResultEventKind::Chunk { chunk })
    }

    pub fn artifact(command: &ActionCommand, artifact: NewRunArtifact) -> Self {
        Self::new(command, WorkflowResultEventKind::Artifact { artifact })
    }

    fn new(command: &ActionCommand, kind: WorkflowResultEventKind) -> Self {
        Self {
            event_id: Uuid::now_v7(),
            command_id: command.command_id,
            workflow_run_id: command.workflow_run_id,
            workflow_node_run_id: command.workflow_node_run_id,
            node_id: command.node_id.clone(),
            attempt: command.attempt,
            kind,
            timestamp: Utc::now(),
            trace_id: command.trace_id,
            notification_delivery_id: command.notification_delivery_id,
            invocation_call_id: command.invocation_call_id,
            task_run_id: command.task_run_id,
        }
    }
}

#[cfg(test)]
mod tests;
