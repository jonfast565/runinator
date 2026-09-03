//! Application service for execution-profile configuration and publication state.

use std::sync::Arc;

use chrono::{DateTime, Utc};
use runinator_models::{
    errors::SendableError,
    execution_profiles::{ExecutionProfile, ExecutionProfileHealth, ExecutionProfileRevision},
};
use runinator_store::roles::ExecutionProfileStore;
use uuid::Uuid;

use crate::repository;

#[derive(Clone)]
pub struct ExecutionProfileOperations<T> {
    store: Arc<T>,
}

impl<T> ExecutionProfileOperations<T> {
    pub fn new(store: Arc<T>) -> Self {
        Self { store }
    }
}

impl<T: ExecutionProfileStore> ExecutionProfileOperations<T> {
    pub async fn list(&self, org_id: Option<Uuid>) -> Result<Vec<ExecutionProfile>, SendableError> {
        repository::list(self.store.as_ref(), org_id).await
    }

    pub async fn fetch(&self, id: Uuid) -> Result<Option<ExecutionProfile>, SendableError> {
        repository::fetch(self.store.as_ref(), id).await
    }

    pub async fn fetch_by_name(
        &self,
        org_id: Option<Uuid>,
        name: &str,
    ) -> Result<Option<ExecutionProfile>, SendableError> {
        repository::fetch_by_name(self.store.as_ref(), org_id, name).await
    }

    pub async fn save(
        &self,
        profile: &ExecutionProfile,
    ) -> Result<ExecutionProfile, SendableError> {
        repository::save(self.store.as_ref(), profile).await
    }

    pub async fn publish_revision(
        &self,
        revision: &ExecutionProfileRevision,
    ) -> Result<ExecutionProfileRevision, SendableError> {
        repository::publish_revision(self.store.as_ref(), revision).await
    }

    pub async fn fetch_revision(
        &self,
        profile_id: Uuid,
        revision: i64,
    ) -> Result<Option<ExecutionProfileRevision>, SendableError> {
        repository::fetch_revision(self.store.as_ref(), profile_id, revision).await
    }

    pub async fn remove(&self, id: Uuid, org_id: Option<Uuid>) -> Result<bool, SendableError> {
        repository::remove(self.store.as_ref(), id, org_id).await
    }

    pub async fn request_refresh(
        &self,
        id: Uuid,
        org_id: Option<Uuid>,
        requested_at: DateTime<Utc>,
    ) -> Result<bool, SendableError> {
        repository::request_refresh(self.store.as_ref(), id, org_id, requested_at).await
    }

    pub async fn update_health(
        &self,
        id: Uuid,
        health: ExecutionProfileHealth,
        error: Option<String>,
    ) -> Result<bool, SendableError> {
        repository::update_health(self.store.as_ref(), id, health, error).await
    }
}
