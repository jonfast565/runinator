use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::{providers::ProviderMetadata, value::Value};

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord)]
#[serde(rename_all = "snake_case")]
pub enum ReplicaKind {
    Worker,
    Waker,
    Webservice,
    Background,
    Postgres,
    Archiver,
}

impl ReplicaKind {
    /// every replica kind, in canonical node-pools display order. this is the single source of
    /// truth for enumerating kinds: adding a variant here surfaces it everywhere that iterates
    /// kinds (provisioner config, supported-kinds, and the node-pools ui) without further edits.
    pub const ALL: &'static [ReplicaKind] = &[
        Self::Webservice,
        Self::Worker,
        Self::Waker,
        Self::Background,
        Self::Archiver,
        Self::Postgres,
    ];

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Worker => "worker",
            Self::Waker => "waker",
            Self::Webservice => "webservice",
            Self::Background => "background",
            Self::Postgres => "postgres",
            Self::Archiver => "archiver",
        }
    }

    /// control-plane kinds back the api or database; scaling one to zero would take the stack down,
    /// so the node-pools ui keeps a floor of one replica for them.
    pub fn is_control_plane(self) -> bool {
        matches!(self, Self::Webservice | Self::Postgres)
    }

    /// the smallest desired count the node-pools ui should allow scaling this kind to.
    pub fn min_desired(self) -> u32 {
        if self.is_control_plane() { 1 } else { 0 }
    }
}

impl TryFrom<&str> for ReplicaKind {
    type Error = String;

    fn try_from(value: &str) -> Result<Self, Self::Error> {
        match value {
            "worker" => Ok(Self::Worker),
            "waker" => Ok(Self::Waker),
            "webservice" => Ok(Self::Webservice),
            "background" => Ok(Self::Background),
            "postgres" => Ok(Self::Postgres),
            "archiver" => Ok(Self::Archiver),
            other => Err(format!("Unknown replica kind '{other}'")),
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ReplicaStatus {
    Live,
    Stale,
    Offline,
}

impl ReplicaStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Live => "live",
            Self::Stale => "stale",
            Self::Offline => "offline",
        }
    }
}

impl TryFrom<&str> for ReplicaStatus {
    type Error = String;

    fn try_from(value: &str) -> Result<Self, Self::Error> {
        match value {
            "live" => Ok(Self::Live),
            "stale" => Ok(Self::Stale),
            "offline" => Ok(Self::Offline),
            other => Err(format!("Unknown replica status '{other}'")),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReplicaRegistrationRequest {
    pub replica_type: ReplicaKind,
    pub instance_id: String,
    pub runtime_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub display_name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub host: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub port: Option<u16>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub base_path: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub version: Option<String>,
    #[serde(default)]
    pub attributes: Value,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReplicaHeartbeatRequest {
    pub runtime_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub display_name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub host: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub port: Option<u16>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub base_path: Option<String>,
    #[serde(default)]
    pub attributes: Value,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReplicaOfflineRequest {
    pub runtime_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReplicaProviderRegistrationRequest {
    pub runtime_id: String,
    pub provider: ProviderMetadata,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReplicaRecord {
    pub replica_id: Uuid,
    pub replica_type: ReplicaKind,
    pub instance_id: String,
    pub runtime_id: String,
    pub status: ReplicaStatus,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub display_name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub host: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub port: Option<u16>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub base_path: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub observed_ip: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub version: Option<String>,
    #[serde(default)]
    pub attributes: Value,
    pub first_seen_at: DateTime<Utc>,
    pub last_heartbeat_at: DateTime<Utc>,
    pub last_seen_at: DateTime<Utc>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub offline_at: Option<DateTime<Utc>>,
    /// the identity that registered this replica, captured once at insert and never reassigned by
    /// later heartbeats/upserts. lets a lower-trust external caller (e.g. a desktop-agent connecting
    /// through the ws broker relay) be checked against the replica_id/labels it presents.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub registered_by_principal_id: Option<Uuid>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub registered_by_kind: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub registered_by_org_id: Option<Uuid>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReplicaProviderRegistration {
    pub replica_id: Uuid,
    pub provider_name: String,
    pub provider: ProviderMetadata,
    pub first_registered_at: DateTime<Utc>,
    pub last_registered_at: DateTime<Utc>,
    pub last_heartbeat_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReplicaCounts {
    pub workers: i64,
    pub wakers: i64,
    pub webservices: i64,
    #[serde(default)]
    pub background: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReplicaListResponse {
    pub counts: ReplicaCounts,
    pub replicas: Vec<ReplicaRecord>,
    /// number of node runs currently executing on each replica, keyed by replica id. derived by the
    /// web service from live executor claims, so only replicas actually running tasks appear.
    #[serde(default)]
    pub running_tasks: std::collections::HashMap<Uuid, i64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct WorkflowRunProvenance {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_kind: Option<TriggerSourceKind>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub actor_type: Option<TriggerActorType>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub actor_replica_id: Option<Uuid>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub actor_display_name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub request_host: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub request_ip: Option<String>,
    #[serde(default)]
    pub metadata: Value,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct WorkflowNodeRunExecutor {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub current_executor_replica_id: Option<Uuid>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_executor_replica_id: Option<Uuid>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub executor_claimed_at: Option<DateTime<Utc>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub executor_released_at: Option<DateTime<Utc>>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum TriggerSourceKind {
    Manual,
    Api,
    Cron,
    System,
    WorkerControl,
    Replay,
    Debug,
    Subflow,
    Map,
}

impl TriggerSourceKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Manual => "manual",
            Self::Api => "api",
            Self::Cron => "cron",
            Self::System => "system",
            Self::WorkerControl => "worker_control",
            Self::Replay => "replay",
            Self::Debug => "debug",
            Self::Subflow => "subflow",
            Self::Map => "map",
        }
    }
}

impl TryFrom<&str> for TriggerSourceKind {
    type Error = String;

    fn try_from(value: &str) -> Result<Self, Self::Error> {
        match value {
            "manual" => Ok(Self::Manual),
            "api" => Ok(Self::Api),
            "cron" => Ok(Self::Cron),
            "system" => Ok(Self::System),
            "worker_control" => Ok(Self::WorkerControl),
            "replay" => Ok(Self::Replay),
            "debug" => Ok(Self::Debug),
            "subflow" => Ok(Self::Subflow),
            "map" => Ok(Self::Map),
            other => Err(format!("Unknown trigger source kind '{other}'")),
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum TriggerActorType {
    User,
    Replica,
    System,
}

impl TriggerActorType {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::User => "user",
            Self::Replica => "replica",
            Self::System => "system",
        }
    }
}

impl TryFrom<&str> for TriggerActorType {
    type Error = String;

    fn try_from(value: &str) -> Result<Self, Self::Error> {
        match value {
            "user" => Ok(Self::User),
            "replica" => Ok(Self::Replica),
            "system" => Ok(Self::System),
            other => Err(format!("Unknown trigger actor type '{other}'")),
        }
    }
}
