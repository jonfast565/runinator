//! Adapter diagnostics and recoverable operator work.
use super::AdapterPollDispatch;
use chrono::{DateTime, Utc};
use runinator_models::{
    adapter_control::{
        AdapterDeliveryRecord, AdapterInspection, AdapterPollAttempt, OrchestrationDebugControl,
    },
    errors::SendableError,
    ingress_control::ExternalIngressRecord,
    value::Value,
};
use std::future::Future;
use uuid::Uuid;

pub trait AdapterControlStore: Send + Sync + 'static {
    fn record_adapter_delivery(
        &self,
        record: AdapterDeliveryRecord,
    ) -> impl Future<Output = Result<AdapterDeliveryRecord, SendableError>> + Send;
    fn fetch_adapter_deliveries(
        &self,
        adapter_id: Uuid,
        limit: i64,
    ) -> impl Future<Output = Result<Vec<AdapterDeliveryRecord>, SendableError>> + Send;
    fn fetch_adapter_delivery(
        &self,
        id: Uuid,
    ) -> impl Future<Output = Result<Option<AdapterDeliveryRecord>, SendableError>> + Send;
    fn update_adapter_delivery(
        &self,
        record: AdapterDeliveryRecord,
    ) -> impl Future<Output = Result<(), SendableError>> + Send;
    fn decide_adapter_delivery(
        &self,
        id: Uuid,
        approve: bool,
    ) -> impl Future<Output = Result<bool, SendableError>> + Send;
    fn release_paused_adapter_deliveries(
        &self,
        adapter_id: Uuid,
        limit: i64,
    ) -> impl Future<Output = Result<(u64, u64), SendableError>> + Send;
    fn claim_adapter_delivery(
        &self,
        token: Uuid,
        now: DateTime<Utc>,
    ) -> impl Future<Output = Result<Option<AdapterDeliveryRecord>, SendableError>> + Send;
    fn finish_adapter_delivery(
        &self,
        record: AdapterDeliveryRecord,
        token: Uuid,
    ) -> impl Future<Output = Result<bool, SendableError>> + Send;
    fn adapter_inspection(
        &self,
        adapter_id: Uuid,
    ) -> impl Future<Output = Result<AdapterInspection, SendableError>> + Send;
    fn set_adapter_inspection(
        &self,
        inspection: AdapterInspection,
    ) -> impl Future<Output = Result<(), SendableError>> + Send;
    fn approve_external_ingress(
        &self,
        id: Uuid,
        actor: Uuid,
        org_id: Option<Uuid>,
    ) -> impl Future<Output = Result<bool, SendableError>> + Send;
    fn claim_approved_external_ingress(
        &self,
        token: Uuid,
        now: DateTime<Utc>,
    ) -> impl Future<Output = Result<Option<ExternalIngressRecord>, SendableError>> + Send;
    fn finish_approved_external_ingress(
        &self,
        id: Uuid,
        token: Uuid,
        error: Option<String>,
        now: DateTime<Utc>,
    ) -> impl Future<Output = Result<bool, SendableError>> + Send;
    fn orchestration_debug_control(
        &self,
        pipeline_id: Uuid,
    ) -> impl Future<Output = Result<OrchestrationDebugControl, SendableError>> + Send;
    fn set_orchestration_debug_control(
        &self,
        pipeline_id: Uuid,
        control: OrchestrationDebugControl,
    ) -> impl Future<Output = Result<(), SendableError>> + Send;
    fn take_orchestration_debug_permit(
        &self,
        pipeline_id: Uuid,
    ) -> impl Future<Output = Result<bool, SendableError>> + Send;
    fn fetch_adapter_poll_attempts(
        &self,
        adapter_id: Uuid,
        limit: i64,
    ) -> impl Future<Output = Result<Vec<AdapterPollAttempt>, SendableError>> + Send;
    fn finish_adapter_poll_attempt(
        &self,
        id: Uuid,
        state: String,
        result: Value,
        error: Option<String>,
        now: DateTime<Utc>,
    ) -> impl Future<Output = Result<bool, SendableError>> + Send;
    fn claim_adapter_poll_publication(
        &self,
        token: Uuid,
        now: DateTime<Utc>,
    ) -> impl Future<Output = Result<Option<AdapterPollDispatch>, SendableError>> + Send;
    fn finish_adapter_poll_publication(
        &self,
        id: Uuid,
        token: Uuid,
        published: bool,
    ) -> impl Future<Output = Result<(), SendableError>> + Send;
    fn expire_adapter_poll_attempts(
        &self,
        now: DateTime<Utc>,
    ) -> impl Future<Output = Result<(), SendableError>> + Send;
    fn purge_adapter_diagnostics(
        &self,
        before: DateTime<Utc>,
    ) -> impl Future<Output = Result<(), SendableError>> + Send;
}
