use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExternalItem {
    pub id: Option<i64>,
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
    pub id: Option<i64>,
    #[serde(default)]
    pub workflow_run_id: Option<i64>,
    #[serde(default)]
    pub external_item_id: Option<i64>,
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
    pub id: Option<i64>,
    #[serde(default)]
    pub workflow_run_id: Option<i64>,
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
    pub id: Option<i64>,
    pub workflow_run_id: i64,
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
    pub id: Option<i64>,
    pub workflow_run_id: i64,
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
    pub id: Option<i64>,
    pub workflow_run_id: i64,
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
    pub id: Option<i64>,
    pub workflow_run_id: i64,
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
    pub id: Option<i64>,
    #[serde(default)]
    pub workflow_run_id: Option<i64>,
    #[serde(default)]
    pub external_item_id: Option<i64>,
    pub provider: String,
    pub event_type: String,
    pub message: String,
    #[serde(default)]
    pub metadata: Value,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CatalogItem {
    pub id: Option<i64>,
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
    pub id: Option<i64>,
    pub scope: String,
    pub key: String,
    #[serde(default)]
    pub result: Value,
    pub created_at: DateTime<Utc>,
}
