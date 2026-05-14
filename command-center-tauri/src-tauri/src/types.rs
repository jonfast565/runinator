use chrono::{DateTime, Utc};

use runinator_models::workflows::{WorkflowNodeRun, WorkflowRun};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ServiceStatus {
    pub service_url: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WebServiceAnnouncement {
    pub service_id: String,
    pub address: String,
    pub port: u16,
    pub base_path: String,
    pub last_heartbeat: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkflowRunDetail {
    pub run: WorkflowRun,
    pub nodes: Vec<WorkflowNodeRun>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkflowBundleSaveRequest {
    pub workflow: runinator_models::workflows::WorkflowDefinition,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkflowBundleSaveResponse {
    pub workflow: runinator_models::workflows::WorkflowDefinition,
    pub tasks: Vec<serde_json::Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkflowRunCreated {
    pub id: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CredentialSummary {
    pub scope: String,
    pub name: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CredentialPutRequest {
    pub scope: String,
    pub name: String,
    pub secret: String,
}
