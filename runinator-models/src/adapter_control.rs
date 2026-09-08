//! Durable adapter diagnostics and operator decisions.

use crate::{
    ingress_control::ExternalIngressGateMode, orchestration::NormalizedAdapterEvent, value::Value,
};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct AdapterOrigin {
    pub adapter_id: Uuid,
    pub revision: i64,
    pub delivery_record_id: Option<Uuid>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AdapterDeliveryRecord {
    #[serde(default)]
    pub approved: bool,
    pub id: Uuid,
    pub origin: AdapterOrigin,
    pub attempt_id: Option<Uuid>,
    pub event: Option<NormalizedAdapterEvent>,
    pub state: String,
    pub error: Option<String>,
    pub preview: Value,
    pub outcome: Value,
    pub received_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AdapterInspection {
    pub adapter_id: Uuid,
    pub mode: ExternalIngressGateMode,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct OrchestrationDebugControl {
    pub paused: bool,
    pub steps: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AdapterPollAttempt {
    pub id: Uuid,
    pub adapter_id: Uuid,
    pub adapter_revision: i64,
    pub dry_run: bool,
    pub state: String,
    pub result: Value,
    pub error: Option<String>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub deadline_at: DateTime<Utc>,
}
