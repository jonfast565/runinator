//! application service for importing compiled workflow packs.

use std::sync::Arc;

use runinator_blob_core::BlobStore;
use runinator_broker_core::{UiEventPublisher, emit_workflows_changed};
use runinator_models::{
    errors::SendableError,
    functions::{FunctionArtifact, FunctionVersion, NewFunctionVersion},
    pipelines::{Pipeline, PipelineBundle},
    workflows::WorkflowBundle,
};
use runinator_store::{
    RuntimeStore,
    roles::{DefinitionStore, FunctionStore, NotificationStore, ScheduleStore},
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
}

impl<T: DefinitionStore + RuntimeStore + FunctionStore + NotificationStore + ScheduleStore>
    PackOperations<T>
{
    pub async fn import_workflows(
        &self,
        bundle: WorkflowBundle,
        overwrite: bool,
    ) -> Result<WorkflowBundle, SendableError> {
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

    pub async fn publish_function(
        &self,
        request: &NewFunctionVersion,
    ) -> Result<FunctionVersion, SendableError> {
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
