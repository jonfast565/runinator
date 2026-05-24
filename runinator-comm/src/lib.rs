pub mod discovery;
pub mod worker_control;

use chrono::{DateTime, Utc};
use runinator_models::{
    runs::{NewRunArtifact, NewRunChunk},
    workflows::{WorkflowAction, WorkflowStatus},
};
use serde::{Deserialize, Serialize};
use serde_json::Value;
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

impl WorkerAnnouncement {
    pub fn to_json(&self) -> serde_json::Result<String> {
        serde_json::to_string(self)
    }

    pub fn from_json(data: &str) -> serde_json::Result<Self> {
        serde_json::from_str(data)
    }
}

impl WebServiceAnnouncement {
    pub fn to_json(&self) -> serde_json::Result<String> {
        serde_json::to_string(self)
    }

    pub fn from_json(data: &str) -> serde_json::Result<Self> {
        serde_json::from_str(data)
    }
}

impl GossipMessage {
    pub fn to_json(&self) -> serde_json::Result<String> {
        serde_json::to_string(self)
    }

    pub fn from_json(data: &str) -> serde_json::Result<Self> {
        serde_json::from_str(data)
    }
}

impl ActionCommand {
    pub fn to_json(&self) -> serde_json::Result<String> {
        serde_json::to_string(self)
    }

    pub fn from_json(data: &str) -> serde_json::Result<Self> {
        serde_json::from_str(data)
    }
}

impl ControlCommand {
    pub fn new(workflow_run_id: i64, kind: ControlKind) -> Self {
        Self {
            workflow_run_id,
            kind,
        }
    }

    pub fn to_json(&self) -> serde_json::Result<String> {
        serde_json::to_string(self)
    }

    pub fn from_json(data: &str) -> serde_json::Result<Self> {
        serde_json::from_str(data)
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

    pub fn to_json(&self) -> serde_json::Result<String> {
        serde_json::to_string(self)
    }

    pub fn from_json(data: &str) -> serde_json::Result<Self> {
        serde_json::from_str(data)
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
