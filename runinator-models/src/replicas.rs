use std::collections::BTreeMap;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::validation::{
    SHORT_TEXT_MAX, Validate, ValidationError, identifier, optional_text, required_text,
};

use crate::{providers::ProviderMetadata, value::Value};

/// routing labels the desktop agent forces on every replica it registers. they identify the
/// runtime rather than describing operator-chosen routing, and the agent refuses to let
/// configuration override them, so an enrolling party cannot choose them either. an enrollment
/// token therefore constrains the labels around these, never these themselves.
pub const DESKTOP_IDENTITY_LABELS: &[(&str, &str)] = &[("pool", "desktop"), ("runner", "desktop")];

/// whether `key`/`value` is one of the immutable desktop identity labels.
pub fn is_desktop_identity_label(key: &str, value: &str) -> bool {
    DESKTOP_IDENTITY_LABELS
        .iter()
        .any(|(label, expected)| *label == key && *expected == value)
}

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
    /// kinds (provisioner config, supported-kinds, and the node-pools UI) without further edits.
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

    /// control-plane kinds back the API or database; scaling one to zero would take the stack down,
    /// so the node-pools UI keeps a floor of one replica for them.
    pub fn is_control_plane(self) -> bool {
        matches!(self, Self::Webservice | Self::Postgres)
    }

    /// the smallest desired count the node-pools UI should allow scaling this kind to.
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

/// lifecycle state reported by an externally hosted worker under `attributes.status`.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum AgentConnectionState {
    #[default]
    Stopped,
    Registering,
    Connecting,
    Connected,
    Draining,
    Reconnecting,
    /// the reconnect budget is spent: the agent gave up and stopped its lifecycle. distinct from
    /// `Stopped`, which is an operator-requested stop.
    Disconnected,
    ReenrollmentRequired,
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
    Chained,
    /// a member workflow run started as part of a pipeline run.
    Pipeline,
    /// a run of a packaged function's adapter workflow, started by a direct http invocation.
    Function,
    /// a run started from a console cell.
    Console,
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
            Self::Chained => "chained",
            Self::Pipeline => "pipeline",
            Self::Function => "function",
            Self::Console => "console",
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
            "chained" => Ok(Self::Chained),
            "pipeline" => Ok(Self::Pipeline),
            "function" => Ok(Self::Function),
            "console" => Ok(Self::Console),
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

mod agent_status_report;
pub use agent_status_report::AgentStatusReport;

mod replica_registration_request;
pub use replica_registration_request::ReplicaRegistrationRequest;

mod replica_heartbeat_request;
pub use replica_heartbeat_request::ReplicaHeartbeatRequest;

mod replica_offline_request;
pub use replica_offline_request::ReplicaOfflineRequest;

mod replica_provider_registration_request;
pub use replica_provider_registration_request::ReplicaProviderRegistrationRequest;

mod replica_record;
pub use replica_record::ReplicaRecord;

mod replica_provider_registration;
pub use replica_provider_registration::ReplicaProviderRegistration;

mod replica_counts;
pub use replica_counts::ReplicaCounts;

mod replica_list_response;
pub use replica_list_response::ReplicaListResponse;

mod workflow_run_provenance;
pub use workflow_run_provenance::WorkflowRunProvenance;

mod workflow_node_run_executor;
pub use workflow_node_run_executor::WorkflowNodeRunExecutor;
