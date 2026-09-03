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

/// Applies pack-owned workflow, function, and pipeline material in the order a pack requires.
#[derive(Clone)]
pub struct PackOperations<T> {
    store: Arc<T>,
    blobs: Arc<dyn BlobStore>,
    events: UiEventPublisher,
}

impl<T> PackOperations<T> {
    pub fn new(store: Arc<T>, blobs: Arc<dyn BlobStore>, events: UiEventPublisher) -> Self {
        Self {
            store,
            blobs,
            events,
        }
    }

    /// Rebind the service to an isolated transactional store while retaining the immutable blob
    /// backend and event publisher. Events are emitted only by the caller after commit.
    fn with_store(&self, store: Arc<T>) -> Self {
        Self {
            store,
            blobs: self.blobs.clone(),
            events: self.events.clone(),
        }
    }
}

impl<T: DefinitionStore + RuntimeStore + FunctionStore + NotificationStore + ScheduleStore>
    PackOperations<T>
{
    pub async fn import_workflows(
        &self,
        bundle: WorkflowBundle,
        overwrite: bool,
    ) -> Result<WorkflowBundle, SendableError>
    where
        T: ExecutionProfileStore,
    {
        repository::import_workflow_bundle_with(self.store.as_ref(), bundle, overwrite).await
    }

    pub async fn put_function_artifact_if_absent(
        &self,
        digest: &str,
        bytes: Vec<u8>,
    ) -> Result<FunctionArtifact, SendableError> {
        repository::functions::put_artifact_if_absent(
            self.store.as_ref(),
            &self.blobs,
            digest,
            bytes,
        )
        .await
    }

    /// Upload immutable bytes before a compiled pack opens its database transaction. The returned
    /// descriptor is persisted by `import_compiled_pack` inside that transaction.
    pub async fn stage_function_artifact(
        &self,
        digest: &str,
        bytes: Vec<u8>,
    ) -> Result<FunctionArtifact, SendableError> {
        repository::functions::stage_artifact(&self.blobs, digest, bytes).await
    }

    pub async fn publish_function(
        &self,
        request: &NewFunctionVersion,
    ) -> Result<FunctionVersion, SendableError>
    where
        T: ExecutionProfileStore,
    {
        repository::functions::publish_version(self.store.as_ref(), request).await
    }

    /// Resolve calls compiled against functions a pack publishes in the same import.
    ///
    /// Local pack compilation uses deterministic temporary IDs because the real version and export
    /// IDs are only minted by `publish_function`. The reserved provisional version makes the
    /// replacement unambiguous: existing bindings are already hard references and are left alone.
    pub async fn resolve_provisional_function_bindings(
        &self,
        bundle: &mut WorkflowBundle,
        published: &[FunctionVersion],
    ) -> Result<(), SendableError> {
        if published.is_empty()
            || !bundle
                .workflows
                .iter()
                .flat_map(|workflow| &workflow.definition.nodes)
                .any(|node| {
                    node.action
                        .iter()
                        .chain(node.compensation.iter())
                        .any(|action| {
                            action
                                .function_binding
                                .as_ref()
                                .is_some_and(|binding| binding.is_provisional())
                        })
                })
        {
            return Ok(());
        }

        let published_versions: std::collections::HashSet<_> =
            published.iter().map(|version| version.id).collect();
        let entries = self
            .store
            .fetch_function_catalog()
            .await?
            .into_iter()
            .filter(|entry| published_versions.contains(&entry.version_id))
            .collect::<Vec<_>>();

        for workflow in &mut bundle.workflows {
            for node in &mut workflow.definition.nodes {
                for action in node.action.iter_mut().chain(node.compensation.iter_mut()) {
                    let Some(binding) = action.function_binding.as_mut() else {
                        continue;
                    };
                    if !binding.is_provisional() {
                        continue;
                    }
                    let candidates = entries
                        .iter()
                        .filter(|entry| {
                            entry.package_name == binding.package_name
                                && entry.namespace == binding.namespace
                                && entry.export_name == binding.export_name
                                && entry.artifact_digest == binding.artifact_digest
                        })
                        .collect::<Vec<_>>();
                    if candidates.len() != 1 {
                        return Err(crate::errors::FUNCTION_NOT_FOUND.error(format!(
                            "node '{}' calls provisional '{}', but pack publish resolved {} matching exports",
                            node.id,
                            binding.call_path(),
                            candidates.len()
                        )));
                    }
                    *binding = candidates[0].binding();
                }
            }
        }
        Ok(())
    }

    pub async fn import_pipelines(
        &self,
        bundle: &PipelineBundle,
        org_id: Option<Uuid>,
    ) -> Result<Vec<Pipeline>, SendableError> {
        repository::import_pipeline_bundle_with(self.store.as_ref(), bundle, org_id).await
    }

    pub fn workflows_changed(&self, org_id: Option<Uuid>) {
        emit_workflows_changed(&self.events, org_id);
    }
}

/// Error classification retained across the engine/HTTP boundary for malformed setting entries.
#[derive(Debug)]
pub struct PackImportError {
    pub bad_request: bool,
    pub message: String,
}

impl PackImportError {
    fn internal(message: impl Into<String>) -> Self {
        Self {
            bad_request: false,
            message: message.into(),
        }
    }
}

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

impl<
    T: DefinitionStore
        + RuntimeStore
        + PackTransactionStore
        + FunctionStore
        + NotificationStore
        + ScheduleStore
        + SettingStore
        + ExecutionProfileStore
        + AuthStore
        + RbacStore,
> PackOperations<T>
{
    /// Apply every mutable part of a compiled pack under one database transaction. Existing role
    /// methods that open transactions execute as savepoints on the transaction store's sole
    /// connection. Blob bytes are deliberately staged by the caller before entering this method.
    #[allow(clippy::too_many_arguments)]
    pub async fn import_compiled_pack(
        &self,
        mut workflows: WorkflowBundle,
        settings: Option<&SettingsBundle>,
        pipelines: Option<&PipelineBundle>,
        functions: &[NewFunctionVersion],
        artifacts: &[FunctionArtifact],
        import_org: Option<Uuid>,
        owner: ScopeRef,
        created_by: Option<Uuid>,
        overwrite: bool,
    ) -> Result<PackImportResult, PackImportError> {
        let transaction = Arc::new(self.store.begin_pack_transaction().await.map_err(|error| {
            PackImportError::internal(format!("could not begin pack transaction: {error}"))
        })?);
        let transactional = self.with_store(transaction.clone());
        let tenant = import_org
            .and_then(|id| ScopeRef::new(runinator_models::rbac::ScopeKind::Organization, Some(id)))
            .unwrap_or(ScopeRef::PLATFORM);

        let applied: Result<PackImportResult, PackImportError> = async {
            for artifact in artifacts {
                transaction
                    .upsert_function_artifact(artifact)
                    .await
                    .map_err(|error| {
                        PackImportError::internal(format!(
                            "pack artifact '{}' could not be recorded: {error}",
                            artifact.digest
                        ))
                    })?;
            }

            let mut published = Vec::with_capacity(functions.len());
            for request in functions {
                let mut request = request.clone();
                request.package.org_id = import_org;
                let version = transactional
                    .publish_function(&request)
                    .await
                    .map_err(|error| {
                        PackImportError::internal(format!(
                            "pack function '{}' could not be published: {error}",
                            request.package.name
                        ))
                    })?;
                ensure_pack_ownership(
                    transaction.as_ref(),
                    ResourceType::FunctionPackage,
                    version.package_id,
                    tenant,
                    owner,
                    created_by,
                )
                .await
                .map_err(|error| PackImportError::internal(error.to_string()))?;
                published.push(version);
            }
            if !published.is_empty() {
                log::info!("Imported {} function versions from pack", published.len());
            }
            transactional
                .resolve_provisional_function_bindings(&mut workflows, &published)
                .await
                .map_err(|error| {
                    PackImportError::internal(format!(
                        "pack function bindings could not be resolved after publish: {error}"
                    ))
                })?;

            let settings = match settings {
                Some(bundle) => SettingsBundle {
                    settings: crate::services::SettingOperations::new(transaction.clone())
                        .import(import_org, bundle, overwrite)
                        .await
                        .map_err(|error| PackImportError {
                            bad_request: error.bad_request,
                            message: error.message,
                        })?,
                    execution_profiles: bundle.execution_profiles.clone(),
                    version: bundle.version,
                },
                None => SettingsBundle::default(),
            };
            let profile_operations =
                crate::services::ExecutionProfileOperations::new(transaction.clone());
            let mut execution_profiles = Vec::with_capacity(settings.execution_profiles.len());
            for entry in &settings.execution_profiles {
                execution_profiles.push(
                    profile_operations
                        .reconcile(
                            import_org,
                            entry.configuration.clone(),
                            entry.updated_at,
                            overwrite,
                        )
                        .await
                        .map_err(|error| PackImportError {
                            bad_request: true,
                            message: error.to_string(),
                        })?,
                );
            }
            for entry in &settings.settings {
                let record = transaction
                    .fetch_setting(
                        import_org,
                        entry.kind,
                        entry.scope.clone(),
                        entry.name.clone(),
                    )
                    .await
                    .map_err(|error| PackImportError::internal(error.to_string()))?
                    .ok_or_else(|| {
                        PackImportError::internal("imported setting could not be reloaded")
                    })?;
                ensure_pack_ownership(
                    transaction.as_ref(),
                    ResourceType::Setting,
                    record.id,
                    tenant,
                    owner,
                    created_by,
                )
                .await
                .map_err(|error| PackImportError::internal(error.to_string()))?;
            }
            for profile in &execution_profiles {
                ensure_pack_ownership(
                    transaction.as_ref(),
                    ResourceType::ExecutionProfile,
                    profile.id,
                    tenant,
                    owner,
                    created_by,
                )
                .await
                .map_err(|error| PackImportError::internal(error.to_string()))?;
            }
            let workflows = transactional
                .import_workflows(workflows, overwrite)
                .await
                .map_err(|error| PackImportError::internal(error.to_string()))?;
            for workflow in &workflows.workflows {
                let Some(workflow_id) = workflow.id else {
                    continue;
                };
                ensure_pack_ownership(
                    transaction.as_ref(),
                    ResourceType::Workflow,
                    workflow_id,
                    tenant,
                    owner,
                    created_by,
                )
                .await
                .map_err(|error| PackImportError::internal(error.to_string()))?;
                if let Some(importing_user) = created_by {
                    for (dependency_type, dependency_id) in
                        crate::repository::workflow_dependency_refs(workflow)
                    {
                        let actor_can_use = runinator_store::resource_access::owner_can_consume(
                            transaction.as_ref(),
                            ScopeRef::new(
                                runinator_models::rbac::ScopeKind::User,
                                Some(importing_user),
                            )
                            .expect("user scope has an id"),
                            tenant,
                            dependency_type,
                            dependency_id,
                        )
                        .await
                        .map_err(|error| PackImportError::internal(error.to_string()))?;
                        if !actor_can_use {
                            return Err(PackImportError {
                                bad_request: true,
                                message: format!(
                                    "importing user is not permitted to use {} {dependency_id}",
                                    dependency_type.as_str()
                                ),
                            });
                        }
                    }
                }
                crate::repository::validate_workflow_dependency_access(
                    transaction.as_ref(),
                    workflow,
                )
                .await
                .map_err(|error| PackImportError {
                    bad_request: true,
                    message: error.to_string(),
                })?;
            }
            let pipelines = match pipelines {
                Some(bundle) => transactional
                    .import_pipelines(bundle, import_org)
                    .await
                    .map_err(|error| PackImportError::internal(error.to_string()))?,
                None => Vec::new(),
            };
            for pipeline in &pipelines {
                if let Some(pipeline_id) = pipeline.id {
                    ensure_pack_ownership(
                        transaction.as_ref(),
                        ResourceType::Pipeline,
                        pipeline_id,
                        tenant,
                        owner,
                        created_by,
                    )
                    .await
                    .map_err(|error| PackImportError::internal(error.to_string()))?;
                }
            }

            Ok(PackImportResult {
                workflows,
                settings,
                execution_profiles: execution_profiles.into_iter().map(Into::into).collect(),
                pipelines,
            })
        }
        .await;

        match applied {
            Ok(result) => {
                if let Err(error) = transaction.commit_pack_transaction().await {
                    let rollback = transaction.rollback_pack_transaction().await;
                    let message = match rollback {
                        Ok(()) => format!("could not commit pack transaction: {error}"),
                        Err(rollback_error) => format!(
                            "could not commit pack transaction: {error}; rollback also failed: {rollback_error}"
                        ),
                    };
                    return Err(PackImportError::internal(message));
                }
                Ok(result)
            }
            Err(error) => {
                if let Err(rollback_error) = transaction.rollback_pack_transaction().await {
                    return Err(PackImportError::internal(format!(
                        "pack import failed and its transaction could not be rolled back: {rollback_error}"
                    )));
                }
                Err(error)
            }
        }
    }
}
