//! Persistence boundary for execution-profile configuration and publication state.

use chrono::{DateTime, Utc};
use runinator_models::{
    errors::SendableError,
    execution_profiles::{ExecutionProfile, ExecutionProfileHealth, ExecutionProfileRevision},
};
use runinator_store::roles::ExecutionProfileStore;
use uuid::Uuid;

pub async fn list<T: ExecutionProfileStore>(
    store: &T,
    org_id: Option<Uuid>,
) -> Result<Vec<ExecutionProfile>, SendableError> {
    store.list_execution_profiles(org_id).await
}

pub async fn fetch<T: ExecutionProfileStore>(
    store: &T,
    id: Uuid,
) -> Result<Option<ExecutionProfile>, SendableError> {
    store.fetch_execution_profile(id).await
}

pub async fn fetch_by_name<T: ExecutionProfileStore>(
    store: &T,
    org_id: Option<Uuid>,
    name: &str,
) -> Result<Option<ExecutionProfile>, SendableError> {
    store.fetch_execution_profile_by_name(org_id, name).await
}

pub async fn save<T: ExecutionProfileStore>(
    store: &T,
    profile: &ExecutionProfile,
) -> Result<ExecutionProfile, SendableError> {
    store.upsert_execution_profile(profile).await
}

pub async fn publish_revision<T: ExecutionProfileStore>(
    store: &T,
    revision: &ExecutionProfileRevision,
) -> Result<ExecutionProfileRevision, SendableError> {
    store.insert_execution_profile_revision(revision).await
}

pub async fn fetch_revision<T: ExecutionProfileStore>(
    store: &T,
    profile_id: Uuid,
    revision: i64,
) -> Result<Option<ExecutionProfileRevision>, SendableError> {
    store
        .fetch_execution_profile_revision(profile_id, revision)
        .await
}

pub async fn remove<T: ExecutionProfileStore>(
    store: &T,
    id: Uuid,
    org_id: Option<Uuid>,
) -> Result<bool, SendableError> {
    store.delete_execution_profile(id, org_id).await
}

pub async fn request_refresh<T: ExecutionProfileStore>(
    store: &T,
    id: Uuid,
    org_id: Option<Uuid>,
    requested_at: DateTime<Utc>,
) -> Result<bool, SendableError> {
    store
        .request_execution_profile_refresh(id, org_id, requested_at)
        .await
}

pub async fn update_health<T: ExecutionProfileStore>(
    store: &T,
    id: Uuid,
    health: ExecutionProfileHealth,
    error: Option<String>,
) -> Result<bool, SendableError> {
    store
        .update_execution_profile_health(id, health, error)
        .await
}
