pub mod discovery;

use chrono::{DateTime, Utc};
use runinator_models::core::ScheduledTask;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkerPeer {
    pub worker_id: Uuid,
    pub address: String,
    pub command_port: u16,
    pub last_heartbeat: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkerAnnouncement {
    pub worker_id: Uuid,
    pub address: String,
    pub command_port: u16,
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
pub struct TaskCommand {
    pub command_id: Uuid,
    pub task: ScheduledTask,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaskResult {
    pub command_id: Uuid,
    pub success: bool,
    pub started_at: DateTime<Utc>,
    pub finished_at: DateTime<Utc>,
    pub message: Option<String>,
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

impl TaskCommand {
    pub fn to_json(&self) -> serde_json::Result<String> {
        serde_json::to_string(self)
    }

    pub fn from_json(data: &str) -> serde_json::Result<Self> {
        serde_json::from_str(data)
    }
}

impl TaskResult {
    pub fn duration_ms(&self) -> i64 {
        (self.finished_at - self.started_at).num_milliseconds()
    }

    pub fn to_json(&self) -> serde_json::Result<String> {
        serde_json::to_string(self)
    }

    pub fn from_json(data: &str) -> serde_json::Result<Self> {
        serde_json::from_str(data)
    }
}
