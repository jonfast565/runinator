//! application service for workflow-definition mutations.
//!
//! A transport authorizes the actor and translates its request; this service performs the
//! definition operation and emits the corresponding broker-backed invalidation only after the
//! durable change succeeds.

use std::sync::Arc;

use runinator_broker_core::{UiEventPublisher, emit_workflows_changed};
use runinator_models::{
    errors::SendableError,
    revisions::RevisionAuthor,
    semver::SemVerBump,
    web::TaskResponse,
    workflows::{WorkflowBundle, WorkflowDefinition, WorkflowRun},
};
use runinator_store::{
    RuntimeStore,
    roles::{
        DefinitionStore, ExecutionProfileStore, FunctionStore, NotificationStore, ScheduleStore,
        WorkflowVmStore,
    },
};
use uuid::Uuid;

use crate::repository;

/// Coordinates authoring mutations and their UI invalidation.
#[derive(Clone)]
pub struct WorkflowAuthoring<T> {
    store: Arc<T>,
    events: UiEventPublisher,
}

impl<T> WorkflowAuthoring<T> {
    pub fn new(store: Arc<T>, events: UiEventPublisher) -> Self {
        Self { store, events }
    }

    fn publish_changed(&self, org_id: Option<Uuid>) {
        emit_workflows_changed(&self.events, org_id);
    }
}

impl<T: DefinitionStore + RuntimeStore + FunctionStore + NotificationStore + ScheduleStore>
    WorkflowAuthoring<T>
{
    /// Validate and persist an authored workflow, recording its revision through the repository
    /// chokepoint before publishing an invalidation.
    pub async fn save(
        &self,
        workflow: &WorkflowDefinition,
        author: &RevisionAuthor,
    ) -> Result<WorkflowDefinition, SendableError>
    where
        T: ExecutionProfileStore,
    {
        let mut workflow = workflow.clone();
        let org_id = workflow.org_id;
        resolve_execution_profiles(
            self.store.as_ref(),
            std::slice::from_mut(&mut workflow),
            org_id,
        )
        .await?;
        let saved = repository::upsert_workflow(self.store.as_ref(), &workflow, author).await?;
        self.publish_changed(saved.org_id);
        Ok(saved)
    }

    pub async fn validate(
        &self,
        workflow: &WorkflowDefinition,
    ) -> Result<WorkflowDefinition, SendableError> {
        repository::validate_workflow_definition_with_catalog(self.store.as_ref(), workflow).await
    }

    /// Import a compiled definition bundle and invalidate the effective authoring scope only once
    /// the complete bundle has been accepted.
    pub async fn import(
        &self,
        bundle: WorkflowBundle,
        overwrite: bool,
        fallback_org_id: Option<Uuid>,
    ) -> Result<WorkflowBundle, SendableError>
    where
        T: ExecutionProfileStore,
    {
        let mut bundle = bundle;
        resolve_execution_profiles(self.store.as_ref(), &mut bundle.workflows, fallback_org_id)
            .await?;
        let imported =
            repository::import_workflow_bundle_with(self.store.as_ref(), bundle, overwrite).await?;
        let org_id = imported
            .workflows
            .first()
            .and_then(|workflow| workflow.org_id)
            .or(fallback_org_id);
        self.publish_changed(org_id);
        Ok(imported)
    }

    pub async fn restore_revision(
        &self,
        workflow_id: Uuid,
        revision: i64,
        author: &RevisionAuthor,
    ) -> Result<WorkflowDefinition, SendableError> {
        let restored = repository::restore_workflow_revision(
            self.store.as_ref(),
            workflow_id,
            revision,
            author,
        )
        .await?;
        self.publish_changed(restored.org_id);
        Ok(restored)
    }

    pub async fn duplicate(
        &self,
        workflow_id: Uuid,
        bump: SemVerBump,
        author: &RevisionAuthor,
        fallback_org_id: Option<Uuid>,
    ) -> Result<WorkflowDefinition, SendableError> {
        let copy =
            repository::duplicate_workflow(self.store.as_ref(), workflow_id, bump, author).await?;
        self.publish_changed(copy.org_id.or(fallback_org_id));
        Ok(copy)
    }

    pub async fn delete(&self, workflow_id: Uuid) -> Result<TaskResponse, SendableError> {
        let org_id = repository::fetch_workflow(self.store.as_ref(), workflow_id)
            .await?
            .and_then(|workflow| workflow.org_id);
        let response = repository::delete_workflow(self.store.as_ref(), workflow_id).await?;
        self.publish_changed(org_id);
        Ok(response)
    }

    pub async fn fetch(
        &self,
        workflow_id: Uuid,
    ) -> Result<Option<WorkflowDefinition>, SendableError> {
        repository::fetch_workflow(self.store.as_ref(), workflow_id).await
    }

    pub async fn fetch_by_name(
        &self,
        name: String,
    ) -> Result<Option<WorkflowDefinition>, SendableError> {
        repository::fetch_workflow_by_name(self.store.as_ref(), name).await
    }

    pub async fn workflow_run(&self, run_id: Uuid) -> Result<Option<WorkflowRun>, SendableError> {
        repository::fetch_workflow_run(self.store.as_ref(), run_id).await
    }

    pub async fn list(&self) -> Result<Vec<WorkflowDefinition>, SendableError> {
        repository::fetch_workflows(self.store.as_ref()).await
    }

    pub async fn export(&self, workflow_id: Option<Uuid>) -> Result<WorkflowBundle, SendableError> {
        repository::export_workflow_bundle(self.store.as_ref(), workflow_id).await
    }

    pub async fn revisions(
        &self,
        workflow_id: Uuid,
        limit: i64,
    ) -> Result<Vec<runinator_models::revisions::WorkflowRevision>, SendableError> {
        repository::fetch_workflow_revisions(self.store.as_ref(), workflow_id, limit).await
    }

    pub async fn revision(
        &self,
        workflow_id: Uuid,
        revision: i64,
    ) -> Result<Option<runinator_models::revisions::WorkflowRevision>, SendableError> {
        repository::fetch_workflow_revision(self.store.as_ref(), workflow_id, revision).await
    }
}

async fn resolve_execution_profiles<T: ExecutionProfileStore + DefinitionStore>(
    store: &T,
    workflows: &mut [WorkflowDefinition],
    fallback_org_id: Option<Uuid>,
) -> Result<(), SendableError> {
    let providers = repository::provider_metadata_from_items(
        repository::fetch_catalog_items(store, Some("provider_metadata".into())).await?,
    )?;
    for workflow in workflows {
        let org_id = workflow.org_id.or(fallback_org_id);
        for node in &mut workflow.definition.nodes {
            for action in node.action.iter_mut().chain(node.compensation.iter_mut()) {
                let Some(binding) = action.execution_profile.as_mut() else {
                    continue;
                };
                let profile = store
                    .fetch_execution_profile_by_name(org_id, &binding.name)
                    .await?
                    .ok_or_else(|| {
                        Box::new(std::io::Error::new(
                            std::io::ErrorKind::NotFound,
                            format!("execution profile '{}' was not found", binding.name),
                        )) as SendableError
                    })?;
                if !profile.enabled {
                    return Err(Box::new(std::io::Error::new(
                        std::io::ErrorKind::PermissionDenied,
                        format!("execution profile '{}' is disabled", binding.name),
                    )));
                }
                if let Some(provider) = providers
                    .iter()
                    .find(|provider| provider.name == action.provider)
                {
                    let missing = provider
                        .metadata
                        .credential_scopes
                        .iter()
                        .filter(|scope| !profile.credential_scopes.contains(scope))
                        .cloned()
                        .collect::<Vec<_>>();
                    if !missing.is_empty() {
                        return Err(Box::new(std::io::Error::new(
                            std::io::ErrorKind::PermissionDenied,
                            format!(
                                "execution profile '{}' does not declare the scopes required by provider '{}': {}",
                                binding.name,
                                action.provider,
                                missing.join(", ")
                            ),
                        )));
                    }
                }
                binding.id = profile.id;
            }
        }
    }
    Ok(())
}

impl<
    T: DefinitionStore
        + RuntimeStore
        + FunctionStore
        + NotificationStore
        + ScheduleStore
        + WorkflowVmStore,
> WorkflowAuthoring<T>
{
    /// Run a dry simulation against live config and optional recorded effect outcomes.
    pub async fn simulate(
        &self,
        workflow: &WorkflowDefinition,
        inputs: runinator_models::value::Value,
        replay_run: Option<Uuid>,
    ) -> Result<runinator_workflows::SimulationRun, SendableError> {
        crate::simulate::simulate_run(self.store.as_ref(), workflow, inputs, replay_run).await
    }
}
