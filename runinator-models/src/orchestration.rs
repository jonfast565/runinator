use crate::{value::Value, workflows::WorkflowStatus};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExternalItem {
    pub id: Option<Uuid>,
    pub provider: String,
    pub resource_type: String,
    pub external_id: String,
    pub status: String,
    #[serde(default)]
    pub title: Option<String>,
    #[serde(default)]
    pub url: Option<String>,
    #[serde(default)]
    pub metadata: Value,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExternalResource {
    pub id: Option<Uuid>,
    #[serde(default)]
    pub workflow_run_id: Option<Uuid>,
    #[serde(default)]
    pub external_item_id: Option<Uuid>,
    pub provider: String,
    pub resource_type: String,
    pub external_id: String,
    pub status: String,
    #[serde(default)]
    pub url: Option<String>,
    #[serde(default)]
    pub metadata: Value,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FeedbackItem {
    pub id: Option<Uuid>,
    #[serde(default)]
    pub workflow_run_id: Option<Uuid>,
    pub provider: String,
    pub resource_type: String,
    pub external_id: String,
    pub status: String,
    pub body: String,
    #[serde(default)]
    pub metadata: Value,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ApprovalRequest {
    pub id: Option<Uuid>,
    pub workflow_run_id: Uuid,
    pub node_id: String,
    pub approval_type: String,
    pub status: String,
    pub prompt: String,
    #[serde(default)]
    pub resolved_by: Option<String>,
    #[serde(default)]
    pub resolved_at: Option<DateTime<Utc>>,
    #[serde(default)]
    pub metadata: Value,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Workspace {
    pub id: Option<Uuid>,
    pub workflow_run_id: Uuid,
    pub provider: String,
    pub resource_type: String,
    pub external_id: String,
    pub status: String,
    pub path: String,
    #[serde(default)]
    pub metadata: Value,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChangeSet {
    pub id: Option<Uuid>,
    pub workflow_run_id: Uuid,
    pub provider: String,
    pub resource_type: String,
    pub external_id: String,
    pub status: String,
    #[serde(default)]
    pub metadata: Value,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GateStatus {
    pub id: Option<Uuid>,
    pub workflow_run_id: Uuid,
    pub provider: String,
    pub resource_type: String,
    pub external_id: String,
    pub status: String,
    #[serde(default)]
    pub metadata: Value,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AutomationEvent {
    pub id: Option<Uuid>,
    #[serde(default)]
    pub workflow_run_id: Option<Uuid>,
    #[serde(default)]
    pub external_item_id: Option<Uuid>,
    pub provider: String,
    pub event_type: String,
    pub message: String,
    #[serde(default)]
    pub metadata: Value,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CatalogItem {
    pub id: Option<Uuid>,
    pub uri: String,
    pub item_type: String,
    pub name: String,
    pub version: String,
    #[serde(default)]
    pub document: Value,
    #[serde(default)]
    pub metadata: Value,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IdempotencyKey {
    pub id: Option<Uuid>,
    pub scope: String,
    pub key: String,
    #[serde(default)]
    pub result: Value,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OrchestrationEvent {
    pub event_id: Uuid,
    pub workflow_run_id: Uuid,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub workflow_node_run_id: Option<Uuid>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub node_id: Option<String>,
    pub event_type: String,
    #[serde(default)]
    pub payload: Value,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NewOrchestrationEvent {
    pub event_id: Uuid,
    pub workflow_run_id: Uuid,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub workflow_node_run_id: Option<Uuid>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub node_id: Option<String>,
    pub event_type: String,
    #[serde(default)]
    pub payload: Value,
    pub created_at: DateTime<Utc>,
}

impl NewOrchestrationEvent {
    pub fn new(
        workflow_run_id: Uuid,
        node_id: Option<String>,
        event_type: impl Into<String>,
        payload: Value,
    ) -> Self {
        Self {
            event_id: Uuid::now_v7(),
            workflow_run_id,
            workflow_node_run_id: None,
            node_id,
            event_type: event_type.into(),
            payload,
            created_at: Utc::now(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReadyNodeRecord {
    pub id: Uuid,
    pub source_event_id: Uuid,
    pub workflow_run_id: Uuid,
    pub node_id: String,
    pub status: WorkflowStatus,
    pub ready_at: DateTime<Utc>,
    pub attempts: i64,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub claimed_by: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub claimed_until: Option<DateTime<Utc>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub completed_at: Option<DateTime<Utc>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReadyNodeClaimRequest {
    pub scheduler_id: String,
    pub lease_until: DateTime<Utc>,
    #[serde(default)]
    pub limit: Option<i64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReadyNodeProcessRequest {
    pub scheduler_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub workflow_run_id: Option<Uuid>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub node_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub next_ready_at: Option<DateTime<Utc>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ActionDispatchClaimRequest {
    pub scheduler_id: String,
    pub lease_until: DateTime<Utc>,
    #[serde(default)]
    pub limit: Option<i64>,
}
