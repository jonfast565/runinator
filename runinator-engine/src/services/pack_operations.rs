//! application service for importing compiled workflow packs.

use std::sync::Arc;

use runinator_blob_core::BlobStore;
use runinator_broker_core::{UiEventPublisher, emit_workflows_changed};
use runinator_models::{
    auth::ResourceType,
    bundles::{PackImportResult, SettingsBundle},
    errors::SendableError,
    functions::{FunctionArtifact, FunctionVersion, NewFunctionVersion},
    pipelines::{Pipeline, PipelineBundle},
    rbac::{ResourceOwnership, ScopeRef},
    workflows::WorkflowBundle,
};
use runinator_store::{
    PackTransactionStore, RuntimeStore,
    roles::{
        AuthStore, DefinitionStore, ExecutionProfileStore, FunctionStore, NotificationStore,
        RbacStore, ScheduleStore, SettingStore,
    },
};
use uuid::Uuid;

use crate::repository;

/// product-owned definitions required before a starter pack is ready to use.

async fn ensure_pack_ownership<T: RbacStore>(
    store: &T,
    resource_type: ResourceType,
    resource_id: Uuid,
    tenant: ScopeRef,
    owner: ScopeRef,
    created_by: Option<Uuid>,
) -> Result<(), SendableError> {
    if store
        .fetch_resource_ownership(resource_type, resource_id)
        .await?
        .is_none()
    {
        let now = chrono::Utc::now();
        store
            .put_resource_ownership(ResourceOwnership {
                resource_type,
                resource_id,
                tenant,
                owner,
                created_by,
                authz_version: 1,
                created_at: now,
                updated_at: now,
            })
            .await?;
    }
    Ok(())
}

mod pack_operations;
pub use pack_operations::PackOperations;

mod pack_import_request;
pub use pack_import_request::PackImportRequest;

mod pack_readiness_request;
pub use pack_readiness_request::PackReadinessRequest;

mod pack_import_error;
pub use pack_import_error::PackImportError;
