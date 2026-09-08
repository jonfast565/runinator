//! Application service for execution-profile configuration and publication state.

use std::sync::Arc;

use chrono::{DateTime, Utc};
use runinator_broker_core::UiEventPublisher;
use runinator_comm::{UiEvent, UiEventKind};
use runinator_models::{
    errors::SendableError,
    execution_profiles::{
        ExecutionProfile, ExecutionProfileAgentStatus, ExecutionProfileCollectionStatus,
        ExecutionProfileHealth, ExecutionProfileOperation,
        ExecutionProfileOperationCompleteRequest, ExecutionProfilePutRequest,
        ExecutionProfileRevision, ExecutionProfileSource, is_portable_environment_name,
        validate_bundle_path, validate_environment_template,
    },
    validation::Validate,
};
use runinator_store::{
    RuntimeStore,
    roles::{DefinitionStore, ExecutionProfileStore, OrchestrationStore},
};
use std::collections::{BTreeMap, HashSet};
use uuid::Uuid;

use crate::repository;

#[derive(Clone)]
pub struct ExecutionProfileOperations<T> {
    store: Arc<T>,
    events: Option<UiEventPublisher>,
}

impl<T> ExecutionProfileOperations<T> {
    pub fn new(store: Arc<T>) -> Self {
        Self {
            store,
            events: None,
        }
    }

    pub fn with_events(mut self, events: UiEventPublisher) -> Self {
        self.events = Some(events);
        self
    }

    fn notify_changed(&self, org_id: Option<Uuid>) {
        if let Some(events) = &self.events {
            events.emit(UiEvent::new(org_id, UiEventKind::ExecutionProfilesChanged));
        }
    }
}

impl<T: ExecutionProfileStore> ExecutionProfileOperations<T> {
    async fn notify_profile_changed(&self, id: Uuid) {
        if self.events.is_none() {
            return;
        }
        match self.fetch(id).await {
            Ok(Some(profile)) => self.notify_changed(profile.org_id),
            Ok(None) => {}
            Err(error) => log::warn!("could not resolve execution-profile event scope: {error}"),
        }
    }

    pub async fn list(&self, org_id: Option<Uuid>) -> Result<Vec<ExecutionProfile>, SendableError> {
        repository::list(self.store.as_ref(), org_id).await
    }

    /// lists platform profiles together with profiles owned by the selected organization.
    pub async fn list_visible_in_scope(
        &self,
        org_id: Option<Uuid>,
    ) -> Result<Vec<ExecutionProfile>, SendableError> {
        let mut profiles = repository::list(self.store.as_ref(), None).await?;
        if let Some(org_id) = org_id {
            profiles.extend(repository::list(self.store.as_ref(), Some(org_id)).await?);
        }
        profiles.sort_by(|left, right| left.name.cmp(&right.name).then(left.id.cmp(&right.id)));
        Ok(profiles)
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
        let saved = repository::save(self.store.as_ref(), profile).await?;
        self.notify_changed(saved.org_id);
        Ok(saved)
    }

    /// Normalize, validate, version, and persist one profile configuration. This is the shared
    /// lifecycle path used by HTTP and pack reconciliation.
    pub async fn configure(
        &self,
        id: Uuid,
        org_id: Option<Uuid>,
        request: ExecutionProfilePutRequest,
        updated_at: Option<DateTime<Utc>>,
        overwrite: bool,
    ) -> Result<ExecutionProfile, SendableError> {
        let request = normalize_profile(request).map_err(invalid_profile)?;
        let existing = self.fetch(id).await?;
        if existing
            .as_ref()
            .is_some_and(|value| value.org_id != org_id)
        {
            return Err(invalid_profile(
                "execution profile belongs to another organization",
            ));
        }
        if let Some(collision) = self.fetch_by_name(org_id, &request.name).await?
            && collision.id != id
        {
            return Err(invalid_profile(
                "an execution profile with this name already exists",
            ));
        }
        let effective_updated_at = updated_at.unwrap_or_else(Utc::now);
        if !overwrite
            && let Some(existing) = existing.as_ref()
            && existing.updated_at >= effective_updated_at
        {
            return Ok(existing.clone());
        }
        let digest = runinator_blob_core::sha256_hex(&serde_json::to_vec(&request)?);
        let changed = existing
            .as_ref()
            .is_some_and(|profile| profile.config_digest != digest);
        let now = effective_updated_at;
        let profile = ExecutionProfile {
            id,
            org_id,
            name: request.name,
            description: request.description,
            credential_scopes: request.credential_scopes,
            collection: request.collection,
            exposure: request.exposure,
            config_version: existing
                .as_ref()
                .map_or(1, |profile| profile.config_version + i64::from(changed)),
            config_digest: digest,
            enabled: request.enabled,
            current_revision: (!changed)
                .then(|| existing.as_ref().and_then(|value| value.current_revision))
                .flatten(),
            current_digest: (!changed)
                .then(|| {
                    existing
                        .as_ref()
                        .and_then(|value| value.current_digest.clone())
                })
                .flatten(),
            current_publisher_id: (!changed)
                .then(|| {
                    existing
                        .as_ref()
                        .and_then(|value| value.current_publisher_id)
                })
                .flatten(),
            published_at: (!changed)
                .then(|| existing.as_ref().and_then(|value| value.published_at))
                .flatten(),
            expires_at: (!changed)
                .then(|| existing.as_ref().and_then(|value| value.expires_at))
                .flatten(),
            refresh_requested_at: existing
                .as_ref()
                .and_then(|value| value.refresh_requested_at),
            health: if !request.enabled {
                ExecutionProfileHealth::Disabled
            } else if changed {
                ExecutionProfileHealth::Unpublished
            } else {
                existing
                    .as_ref()
                    .map_or(ExecutionProfileHealth::Unpublished, |value| value.health)
            },
            last_error: if changed {
                None
            } else {
                existing.as_ref().and_then(|value| value.last_error.clone())
            },
            created_at: existing.as_ref().map_or(now, |value| value.created_at),
            updated_at: now,
        };
        self.save(&profile).await
    }

    /// Reconcile a named pack entry, assigning a server UUID for a new name.
    pub async fn reconcile(
        &self,
        org_id: Option<Uuid>,
        request: ExecutionProfilePutRequest,
        updated_at: Option<DateTime<Utc>>,
        overwrite: bool,
    ) -> Result<ExecutionProfile, SendableError> {
        let name = request.name.trim();
        let id = self
            .fetch_by_name(org_id, name)
            .await?
            .map_or_else(Uuid::new_v4, |profile| profile.id);
        self.configure(id, org_id, request, updated_at, overwrite)
            .await
    }

    pub async fn publish_revision(
        &self,
        revision: &ExecutionProfileRevision,
    ) -> Result<ExecutionProfileRevision, SendableError> {
        let published = repository::publish_revision(self.store.as_ref(), revision).await?;
        self.notify_profile_changed(revision.profile_id).await;
        Ok(published)
    }

    pub async fn fetch_revision(
        &self,
        profile_id: Uuid,
        revision: i64,
    ) -> Result<Option<ExecutionProfileRevision>, SendableError> {
        repository::fetch_revision(self.store.as_ref(), profile_id, revision).await
    }

    pub async fn remove(&self, id: Uuid, org_id: Option<Uuid>) -> Result<bool, SendableError> {
        let removed = repository::remove(self.store.as_ref(), id, org_id).await?;
        if removed {
            self.notify_changed(org_id);
        }
        Ok(removed)
    }

    pub async fn request_refresh(
        &self,
        id: Uuid,
        org_id: Option<Uuid>,
        requested_at: DateTime<Utc>,
    ) -> Result<bool, SendableError> {
        let changed =
            repository::request_refresh(self.store.as_ref(), id, org_id, requested_at).await?;
        if changed {
            self.notify_changed(org_id);
        }
        Ok(changed)
    }

    pub async fn update_health(
        &self,
        id: Uuid,
        health: ExecutionProfileHealth,
        error: Option<String>,
    ) -> Result<bool, SendableError> {
        let changed = repository::update_health(self.store.as_ref(), id, health, error).await?;
        if changed {
            self.notify_profile_changed(id).await;
        }
        Ok(changed)
    }

    /// Build the author-facing collection status without allowing desktop reports to mutate the
    /// immutable publication state.
    pub async fn collection_status(
        &self,
        profile: &ExecutionProfile,
    ) -> Result<ExecutionProfileCollectionStatus, SendableError> {
        Ok(ExecutionProfileCollectionStatus {
            profile_id: profile.id,
            config_digest: profile.config_digest.clone(),
            publication_health: profile.health,
            current_revision: profile.current_revision,
            published_at: profile.published_at,
            expires_at: profile.expires_at,
            latest_operation: self
                .store
                .fetch_latest_execution_profile_operation(profile.id, &profile.config_digest)
                .await?,
            agents: self
                .store
                .list_execution_profile_agent_statuses(profile.id, &profile.config_digest)
                .await?,
        })
    }

    pub async fn record_agent_status(
        &self,
        status: &ExecutionProfileAgentStatus,
    ) -> Result<(), SendableError> {
        self.store
            .upsert_execution_profile_agent_status(status)
            .await?;
        self.notify_profile_changed(status.profile_id).await;
        Ok(())
    }

    pub async fn request_operation(
        &self,
        operation: &ExecutionProfileOperation,
    ) -> Result<ExecutionProfileOperation, SendableError> {
        let operation = self
            .store
            .insert_execution_profile_operation(operation)
            .await?;
        self.notify_profile_changed(operation.profile_id).await;
        Ok(operation)
    }

    pub async fn latest_operation(
        &self,
        profile_id: Uuid,
        config_digest: &str,
    ) -> Result<Option<ExecutionProfileOperation>, SendableError> {
        self.store
            .fetch_latest_execution_profile_operation(profile_id, config_digest)
            .await
    }

    pub async fn pending_operations(
        &self,
        org_id: Option<Uuid>,
    ) -> Result<Vec<ExecutionProfileOperation>, SendableError> {
        self.store
            .list_pending_execution_profile_operations(org_id, Utc::now())
            .await
    }

    pub async fn claim_operation(
        &self,
        operation_id: Uuid,
        agent_id: Uuid,
        config_digest: &str,
    ) -> Result<Option<ExecutionProfileOperation>, SendableError> {
        let started_at = Utc::now();
        let operation = self
            .store
            .claim_execution_profile_operation(
                operation_id,
                agent_id,
                config_digest,
                started_at,
                started_at + chrono::Duration::minutes(30),
            )
            .await?;
        if let Some(operation) = &operation {
            self.notify_profile_changed(operation.profile_id).await;
        }
        Ok(operation)
    }

    pub async fn complete_operation(
        &self,
        operation_id: Uuid,
        agent_id: Uuid,
        org_id: Option<Uuid>,
        request: ExecutionProfileOperationCompleteRequest,
    ) -> Result<bool, SendableError> {
        let changed = self
            .store
            .complete_execution_profile_operation(
                operation_id,
                agent_id,
                request.state,
                request.error,
                Utc::now(),
            )
            .await?;
        if changed {
            self.notify_changed(org_id);
        }
        Ok(changed)
    }
}

impl<T: RuntimeStore> ExecutionProfileOperations<T> {
    /// Check that a profile was frozen into the admitted workflow snapshot.
    pub async fn run_admitted_profile(
        &self,
        run_id: Uuid,
        profile_id: Uuid,
    ) -> Result<bool, SendableError> {
        Ok(self
            .store
            .fetch_workflow_run(run_id)
            .await?
            .and_then(|run| run.workflow_snapshot)
            .is_some_and(|workflow| {
                crate::repository::workflow_dependency_refs(&workflow).contains(&(
                    runinator_models::auth::ResourceType::ExecutionProfile,
                    profile_id,
                ))
            }))
    }
}

impl<T: DefinitionStore + ExecutionProfileStore> ExecutionProfileOperations<T> {
    /// Return the stored workflow paths that bind this profile. Both durable UUID bindings and
    /// unresolved authored aliases are included so deletion cannot strand a disabled workflow.
    pub async fn dependent_workflow_paths(
        &self,
        id: Uuid,
        org_id: Option<Uuid>,
        name: &str,
    ) -> Result<Vec<String>, SendableError> {
        Ok(self
            .store
            .fetch_workflows()
            .await?
            .into_iter()
            .filter(|workflow| {
                workflow.org_id == org_id
                    && workflow.definition.nodes.iter().any(|node| {
                        node.action
                            .iter()
                            .chain(node.compensation.iter())
                            .any(|action| {
                                action.execution_profile.as_ref().is_some_and(|binding| {
                                    binding.id() == id
                                        || (binding.id().is_nil() && binding.name() == name)
                                })
                            })
                    })
            })
            .map(|workflow| workflow.artifact_path().qualified())
            .collect())
    }
}

impl<T: OrchestrationStore> ExecutionProfileOperations<T> {
    /// Return active adapters whose current revision binds this profile. Durable UUID bindings and
    /// unresolved authored aliases are both included so deletion cannot strand adapter polling.
    pub async fn dependent_adapter_names(
        &self,
        id: Uuid,
        org_id: Option<Uuid>,
        name: &str,
    ) -> Result<Vec<String>, SendableError> {
        let mut dependents = Vec::new();
        for adapter in self.store.fetch_orchestration_adapters(org_id).await? {
            let Some(revision) = self
                .store
                .fetch_orchestration_adapter_revision(adapter.id, adapter.current_revision)
                .await?
            else {
                continue;
            };
            if revision
                .authentication
                .execution_profile()
                .is_some_and(|binding| {
                    binding.id() == id || (binding.id().is_nil() && binding.name() == name)
                })
            {
                dependents.push(adapter.name);
            }
        }
        dependents.sort();
        Ok(dependents)
    }
}

fn invalid_profile(message: impl Into<String>) -> SendableError {
    Box::new(std::io::Error::new(
        std::io::ErrorKind::InvalidInput,
        message.into(),
    ))
}

pub fn normalize_profile(
    mut request: ExecutionProfilePutRequest,
) -> Result<ExecutionProfilePutRequest, String> {
    request.name = request.name.trim().to_string();
    request.description = request.description.trim().to_string();
    if request.name.is_empty() {
        return Err("profile name is required".into());
    }
    if request.collection.version != 1 || request.exposure.version != 1 {
        return Err(
            "only execution profile collection/exposure specification version 1 is supported"
                .into(),
        );
    }
    let mut scopes = HashSet::new();
    for scope in &mut request.credential_scopes {
        *scope = scope.trim().to_string();
        if scope.is_empty() || !scopes.insert(scope.to_ascii_lowercase()) {
            return Err("credential scopes must be non-blank and unique ignoring case".into());
        }
    }
    request
        .credential_scopes
        .sort_by_key(|scope| scope.to_ascii_lowercase());
    request
        .credential_scopes
        .dedup_by(|left, right| left.eq_ignore_ascii_case(right));
    if request.credential_scopes.is_empty() {
        return Err("at least one credential scope is required".into());
    }
    for (label, command) in [
        ("probe", request.collection.probe.as_mut()),
        ("refresh", request.collection.refresh.as_mut()),
    ] {
        if let Some(command) = command {
            normalize_command(command, label)?;
            if label == "probe" && command.interactive {
                return Err("probe commands cannot be interactive".into());
            }
        }
    }
    let mut targets = HashSet::new();
    for source in &mut request.collection.sources {
        let target = match source {
            ExecutionProfileSource::File { path, target }
            | ExecutionProfileSource::Directory { path, target, .. } => {
                *path = path.trim().to_string();
                target
            }
            ExecutionProfileSource::Command { command, target } => {
                normalize_command(command, "collection")?;
                if command.interactive {
                    return Err("command sources cannot be interactive".into());
                }
                target
            }
        };
        *target = target.trim().to_string();
        validate_bundle_path(target)?;
        if !targets.insert(target.clone()) {
            return Err(format!("duplicate bundle target '{target}'"));
        }
        if let ExecutionProfileSource::Directory { glob, .. } = source {
            *glob = glob.trim().to_string();
            if glob.is_empty() {
                return Err("directory source glob cannot be blank".into());
            }
        }
    }
    if request.collection.sources.is_empty() {
        return Err("at least one collection source is required".into());
    }
    let mut environment = BTreeMap::new();
    let mut environment_names = HashSet::new();
    for (key, value) in std::mem::take(&mut request.exposure.environment) {
        let key = key.trim().to_string();
        let value = value.trim().to_string();
        if !is_portable_environment_name(&key) {
            return Err(format!("invalid portable environment name '{key}'"));
        }
        if !environment_names.insert(key.to_ascii_lowercase()) {
            return Err(format!("duplicate environment name '{key}' ignoring case"));
        }
        validate_environment_template(&value)?;
        environment.insert(key, value);
    }
    request.exposure.environment = environment;
    request.validate().map_err(|error| error.to_string())?;
    Ok(request)
}

fn normalize_command(
    command: &mut runinator_models::execution_profiles::ExecutionProfileCommand,
    label: &str,
) -> Result<(), String> {
    for value in &mut command.argv {
        *value = value.trim().to_string();
    }
    if command.argv.is_empty() || command.argv.iter().any(String::is_empty) {
        return Err(format!("{label} command argv cannot be empty"));
    }
    Ok(())
}

#[cfg(test)]
#[path = "execution_profile_operations_tests.rs"]
mod tests;
