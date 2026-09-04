//! Durable execution-profile definitions and encrypted bundle revision metadata.

use std::future::Future;

use runinator_models::{
    errors::SendableError,
    execution_profiles::{
        ExecutionProfile, ExecutionProfileAgentStatus, ExecutionProfileOperation,
        ExecutionProfileOperationState, ExecutionProfileRevision,
    },
};
use uuid::Uuid;

pub trait ExecutionProfileStore: Send + Sync + 'static {
    fn upsert_execution_profile(
        &self,
        profile: &ExecutionProfile,
    ) -> impl Future<Output = Result<ExecutionProfile, SendableError>> + Send;
    fn list_execution_profiles(
        &self,
        org_id: Option<Uuid>,
    ) -> impl Future<Output = Result<Vec<ExecutionProfile>, SendableError>> + Send;
    fn fetch_execution_profile(
        &self,
        id: Uuid,
    ) -> impl Future<Output = Result<Option<ExecutionProfile>, SendableError>> + Send;
    fn fetch_execution_profile_by_name(
        &self,
        org_id: Option<Uuid>,
        name: &str,
    ) -> impl Future<Output = Result<Option<ExecutionProfile>, SendableError>> + Send;
    fn insert_execution_profile_revision(
        &self,
        revision: &ExecutionProfileRevision,
    ) -> impl Future<Output = Result<ExecutionProfileRevision, SendableError>> + Send;
    fn fetch_execution_profile_revision(
        &self,
        profile_id: Uuid,
        revision: i64,
    ) -> impl Future<Output = Result<Option<ExecutionProfileRevision>, SendableError>> + Send;
    fn delete_execution_profile(
        &self,
        id: Uuid,
        org_id: Option<Uuid>,
    ) -> impl Future<Output = Result<bool, SendableError>> + Send;

    fn request_execution_profile_refresh(
        &self,
        id: Uuid,
        org_id: Option<Uuid>,
        requested_at: chrono::DateTime<chrono::Utc>,
    ) -> impl Future<Output = Result<bool, SendableError>> + Send;

    fn update_execution_profile_health(
        &self,
        id: Uuid,
        health: runinator_models::execution_profiles::ExecutionProfileHealth,
        error: Option<String>,
    ) -> impl Future<Output = Result<bool, SendableError>> + Send;

    fn upsert_execution_profile_agent_status(
        &self,
        status: &ExecutionProfileAgentStatus,
    ) -> impl Future<Output = Result<(), SendableError>> + Send;

    fn list_execution_profile_agent_statuses(
        &self,
        profile_id: Uuid,
        config_digest: &str,
    ) -> impl Future<Output = Result<Vec<ExecutionProfileAgentStatus>, SendableError>> + Send;

    fn insert_execution_profile_operation(
        &self,
        operation: &ExecutionProfileOperation,
    ) -> impl Future<Output = Result<ExecutionProfileOperation, SendableError>> + Send;

    fn fetch_latest_execution_profile_operation(
        &self,
        profile_id: Uuid,
        config_digest: &str,
    ) -> impl Future<Output = Result<Option<ExecutionProfileOperation>, SendableError>> + Send;

    fn list_pending_execution_profile_operations(
        &self,
        org_id: Option<Uuid>,
        now: chrono::DateTime<chrono::Utc>,
    ) -> impl Future<Output = Result<Vec<ExecutionProfileOperation>, SendableError>> + Send;

    fn claim_execution_profile_operation(
        &self,
        operation_id: Uuid,
        agent_id: Uuid,
        config_digest: &str,
        started_at: chrono::DateTime<chrono::Utc>,
        lease_expires_at: chrono::DateTime<chrono::Utc>,
    ) -> impl Future<Output = Result<Option<ExecutionProfileOperation>, SendableError>> + Send;

    fn complete_execution_profile_operation(
        &self,
        operation_id: Uuid,
        agent_id: Uuid,
        state: ExecutionProfileOperationState,
        error: Option<String>,
        completed_at: chrono::DateTime<chrono::Utc>,
    ) -> impl Future<Output = Result<bool, SendableError>> + Send;
}
