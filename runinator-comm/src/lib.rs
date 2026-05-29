pub mod discovery;
pub mod wire;
pub mod worker_control;

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
    pub workflow_run_id: i64,
    pub workflow_node_run_id: i64,
    pub node_id: String,
    pub action: WorkflowAction,
    pub attempt: i64,
    #[serde(default)]
    pub parameters: Value,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ActionDispatchRecord {
    pub id: i64,
    pub dedupe_key: String,
    pub command: ActionCommand,
    pub attempts: i64,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub published_at: Option<DateTime<Utc>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_error: Option<String>,
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
    pub workflow_run_id: i64,
    pub kind: ControlKind,
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
    pub workflow_run_id: i64,
    #[serde(flatten)]
    pub verb: DebugVerb,
}

impl DebugCommand {
    pub fn new(workflow_run_id: i64, verb: DebugVerb) -> Self {
        Self {
            workflow_run_id,
            verb,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkflowResultEvent {
    pub event_id: Uuid,
    pub command_id: Uuid,
    pub workflow_run_id: i64,
    pub workflow_node_run_id: i64,
    pub node_id: String,
    pub kind: WorkflowResultEventKind,
    pub timestamp: DateTime<Utc>,
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
    pub fn new(workflow_run_id: i64, kind: ControlKind) -> Self {
        Self {
            workflow_run_id,
            kind,
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
            event_id: Uuid::new_v4(),
            command_id: command.command_id,
            workflow_run_id: command.workflow_run_id,
            workflow_node_run_id: command.workflow_node_run_id,
            node_id: command.node_id.clone(),
            kind,
            timestamp: Utc::now(),
        }
    }
}

#[cfg(test)]
mod tests;
