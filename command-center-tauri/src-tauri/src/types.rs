use chrono::{DateTime, Utc};
use std::collections::HashMap;

use runinator_models::{
    core::ScheduledTask,
    workflows::{WorkflowNodeRun, WorkflowRun},
};
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
pub struct SaveTaskRequest {
    pub task: ScheduledTask,
    pub creating: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SaveTaskResponse {
    pub success: bool,
    pub message: String,
    pub creating: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkflowBundleTaskDraft {
    pub node_id: String,
    pub temporary_id: Option<i64>,
    pub task: ScheduledTask,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkflowBundleSaveRequest {
    pub workflow: runinator_models::workflows::WorkflowDefinition,
    pub tasks: Vec<WorkflowBundleTaskDraft>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkflowBundleSaveResponse {
    pub workflow: runinator_models::workflows::WorkflowDefinition,
    pub task_id_map: HashMap<String, i64>,
    pub tasks: Vec<ScheduledTask>,
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
