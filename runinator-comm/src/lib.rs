pub mod discovery;
pub mod errors;
pub mod wire;

pub use wire::{WireCodec, WireError};

use chrono::{DateTime, Utc};
use runinator_models::{
    runs::{NewRunArtifact, NewRunChunk},
    value::Value,
    workflow_state::DebugMode,
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
    pub last_heartbeat: DateTime<Utc>,
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
    /// correlation id propagated across the ws -> broker -> worker hop so spans/logs for one action
    /// execution line up. defaults for backward-compatible deserialization of older messages.
    #[serde(default = "Uuid::now_v7")]
    pub trace_id: Uuid,
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
}

/// a request to run the web-service reducer for one ready-queue row at a future time. the web
/// service publishes this when it enqueues a ready node (and the reconcile backstop re-publishes
/// overdue ones); the waker is the sole consumer and relays a [`WsIngressCommand::Drive`] once
/// `ready_at` arrives.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WakeCommand {
    pub ready_node_id: Uuid,
    pub workflow_run_id: Uuid,
    pub node_id: String,
    pub ready_at: DateTime<Utc>,
    pub source_event_id: Uuid,
}

impl WakeCommand {
    pub fn new(
        ready_node_id: Uuid,
        workflow_run_id: Uuid,
        node_id: String,
        ready_at: DateTime<Utc>,
        source_event_id: Uuid,
    ) -> Self {
        Self {
            ready_node_id,
            workflow_run_id,
            node_id,
            ready_at,
            source_event_id,
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
    },
    /// worker -> ws: a control request raised by an executing action.
    Control {
        workflow_run_id: Uuid,
        kind: ControlKind,
    },
}

impl WsIngressCommand {
    pub fn drive(ready_node_id: Uuid, workflow_run_id: Uuid, node_id: String) -> Self {
        Self::Drive {
            ready_node_id,
            workflow_run_id,
            node_id,
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
        }
    }
}

/// the canonical set of debugger operations against a run. one tagged contract replaces the prior
/// per-endpoint shapes so every layer (frontend, web service, future broker paths) names debug
/// operations identically. payload-carrying verbs (skip/rerun/set_*) live here rather than on the
/// unit-variant [`ControlKind`].
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "verb", rename_all = "snake_case")]
pub enum DebugVerb {
    /// advance exactly one node, then pause again.
    Step,
    /// resume normal execution (still honoring breakpoints).
    Continue,
    /// run until `cursor` is reached, pausing there once.
    RunToCursor { cursor: String },
    /// mark the active node succeeded with a synthetic `output` and advance.
    Skip {
        #[serde(default)]
        output: Value,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        message: Option<String>,
    },
    /// supersede the active node's latest attempt and re-execute it with `parameters`.
    Rerun {
        #[serde(default)]
        parameters: Value,
    },
    /// replace the configured breakpoint set.
    SetBreakpoints { breakpoints: Vec<String> },
    /// set the step granularity.
    SetMode { mode: DebugMode },
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
/// on the broker fan-out `events` channel (every ws pod receives every event).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum UiEvent {
    RunStatusChanged { run_id: Uuid, terminal: bool },
    RunChunkAdded { run_id: Uuid },
    WorkflowsChanged,
    WorkflowRunChanged { run_id: Uuid },
    WorkflowRunActivity,
    TasksChanged,
    ArtifactCreated { artifact_id: Uuid, run_id: Uuid },
    NotificationCreated { notification_id: Uuid },
    NotificationsChanged,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkflowResultEvent {
    pub event_id: Uuid,
    pub command_id: Uuid,
    pub workflow_run_id: Uuid,
    pub workflow_node_run_id: Uuid,
    pub node_id: String,
    pub kind: WorkflowResultEventKind,
    pub timestamp: DateTime<Utc>,
    /// correlation id carried back from the originating [`ActionCommand`] so worker result handling
    /// stays on the same trace. defaults for backward-compatible deserialization of older messages.
    #[serde(default = "Uuid::now_v7")]
    pub trace_id: Uuid,
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
        }
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
            kind,
            timestamp: Utc::now(),
            trace_id: command.trace_id,
        }
    }
}

#[cfg(test)]
mod tests;
