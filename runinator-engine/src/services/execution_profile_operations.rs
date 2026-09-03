//! Application service for execution-profile configuration and publication state.

use std::sync::Arc;

use chrono::{DateTime, Utc};
use runinator_models::{
    errors::SendableError,
    execution_profiles::{
        ExecutionProfile, ExecutionProfileHealth, ExecutionProfilePutRequest,
        ExecutionProfileRevision, ExecutionProfileSource, is_portable_environment_name,
        validate_bundle_path, validate_environment_template,
    },
    validation::Validate,
};
use runinator_store::roles::{DefinitionStore, ExecutionProfileStore};
use std::collections::{BTreeMap, HashSet};
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
mod tests {
    use super::*;
    use runinator_database::sqlite::SqliteDb;
    use runinator_models::execution_profiles::{
        ExecutionProfileCollectionSpec, ExecutionProfileExposureSpec,
    };
    use runinator_store::DatabaseImpl;

    fn request() -> ExecutionProfilePutRequest {
        ExecutionProfilePutRequest {
            name: " github-default ".into(),
            description: " GitHub login ".into(),
            credential_scopes: vec!["github".into(), " copilot ".into()],
            collection: ExecutionProfileCollectionSpec {
                sources: vec![ExecutionProfileSource::File {
                    path: " ~/.gitconfig ".into(),
                    target: " .gitconfig ".into(),
                }],
                ..Default::default()
            },
            exposure: ExecutionProfileExposureSpec {
                home_overlay: true,
                environment: BTreeMap::from([(
                    " GH_CONFIG_DIR ".into(),
                    " ${PROFILE_HOME}/.config/gh ".into(),
                )]),
                ..Default::default()
            },
            enabled: true,
        }
    }

    #[test]
    fn canonical_profile_normalization_is_stable() {
        let normalized = normalize_profile(request()).expect("valid profile");
        assert_eq!(normalized.name, "github-default");
        assert_eq!(normalized.description, "GitHub login");
        assert_eq!(normalized.credential_scopes, ["copilot", "github"]);
        assert_eq!(
            normalized.exposure.environment.get("GH_CONFIG_DIR"),
            Some(&"${PROFILE_HOME}/.config/gh".to_string())
        );
        assert_eq!(
            normalize_profile(normalized.clone()).expect("idempotent"),
            normalized
        );
    }

    #[test]
    fn canonical_profile_rejects_ambiguous_scopes_and_targets() {
        let mut duplicate_scope = request();
        duplicate_scope.credential_scopes = vec!["GitHub".into(), "github".into()];
        assert!(normalize_profile(duplicate_scope).is_err());

        let mut duplicate_target = request();
        duplicate_target
            .collection
            .sources
            .push(ExecutionProfileSource::File {
                path: "~/.config/gh".into(),
                target: ".gitconfig".into(),
            });
        assert!(normalize_profile(duplicate_target).is_err());
    }

    #[tokio::test]
    async fn unchanged_configuration_preserves_publication_and_changes_invalidate_it() {
        let path = std::env::temp_dir().join(format!("runinator-profile-{}.db", Uuid::now_v7()));
        let db = Arc::new(SqliteDb::new(path.to_str().unwrap()).await.unwrap());
        db.run_init_scripts(&Vec::new()).await.unwrap();
        let service = ExecutionProfileOperations::new(db);
        let id = Uuid::new_v4();
        let configured = service
            .configure(id, None, request(), Some(Utc::now()), true)
            .await
            .unwrap();
        service
            .publish_revision(&ExecutionProfileRevision {
                profile_id: id,
                revision: 1,
                digest: "archive".into(),
                size_bytes: 7,
                publisher_id: None,
                expires_at: None,
                created_at: configured.updated_at,
                uri: "blob://profile".into(),
            })
            .await
            .unwrap();

        let unchanged = service
            .configure(
                id,
                None,
                request(),
                Some(configured.updated_at + chrono::Duration::seconds(1)),
                true,
            )
            .await
            .unwrap();
        assert_eq!(unchanged.config_version, 1);
        assert_eq!(unchanged.current_revision, Some(1));

        let mut changed_request = request();
        changed_request.description = "Changed".into();
        let changed = service
            .configure(
                id,
                None,
                changed_request,
                Some(configured.updated_at + chrono::Duration::seconds(2)),
                true,
            )
            .await
            .unwrap();
        assert_eq!(changed.config_version, 2);
        assert_eq!(changed.health, ExecutionProfileHealth::Unpublished);
        assert_eq!(changed.current_revision, None);
        assert_eq!(changed.current_digest, None);

        let _ = std::fs::remove_file(path);
    }
}
