//! Durable execution-profile definitions and encrypted bundle revision metadata.

use std::future::Future;

use runinator_models::{
    errors::SendableError,
    execution_profiles::{ExecutionProfile, ExecutionProfileRevision},
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
}
