//! Restore and snapshot isolated portable workspaces around provider execution.
use runinator_api::{AsyncApiClient, StaticLocator};
use runinator_models::{
    errors::{SendableError, WORKSPACE_INVALID},
    value::Value,
    workspaces::*,
};

pub struct ActiveWorkspace {
    execution: WorkspaceExecution,
    replica_id: uuid::Uuid,
    directory: Option<tempfile::TempDir>,
    path: std::path::PathBuf,
    results: std::collections::BTreeMap<String, Value>,
    objects: std::sync::Arc<super::workspace_objects::WorkerObjects>,
}

impl ActiveWorkspace {
    pub async fn restore(
        api: &AsyncApiClient<StaticLocator>,
        value: &Value,
        replica_id: uuid::Uuid,
        deadline: std::time::Instant,
    ) -> Result<Self, SendableError> {
        let execution: WorkspaceExecution = value.decode()?;
        let expires = execution.checkout.leased_until.timestamp();
        let root = cache_root()?;
        let revision_id = execution
            .snapshot
            .as_ref()
            .map(|snapshot| snapshot.revision_id.clone());
        let archive = if revision_id.is_some() {
            let remaining = deadline
                .checked_duration_since(std::time::Instant::now())
                .ok_or_else(|| {
                    runinator_workspace::storage::Error::Io(std::io::Error::new(
                        std::io::ErrorKind::TimedOut,
                        "workspace attempt deadline exceeded",
                    ))
                })?;
            Some(
                tokio::time::timeout(
                    remaining,
                    api.download_workspace_checkout(execution.checkout.id, replica_id, remaining),
                )
                .await??,
            )
        } else {
            None
        };
        let api = api.clone();
        let checkout = execution.checkout.id;
        let limits = execution.checkout.limits;
        let (directory, results, objects) =
            tokio::task::spawn_blocking(move || -> Result<_, SendableError> {
                std::fs::create_dir_all(&root)?;
                let directory = tempfile::Builder::new()
                    .prefix(&format!("lease-{expires}-"))
                    .tempdir_in(root)?;
                let local = if let Some(bytes) = archive {
                    let scratch = tempfile::tempdir()?;
                    let (local, revision, _) = runinator_workspace::native::import(
                        bytes.as_slice(),
                        scratch.path(),
                        limits,
                    )?;
                    if Some(revision.to_string()) != revision_id {
                        return Err(WORKSPACE_INVALID
                            .error("checkout archive contains a different revision"));
                    }
                    Some(local)
                } else {
                    None
                };
                let objects = std::sync::Arc::new(super::workspace_objects::WorkerObjects::new(
                    api, checkout, replica_id, deadline, local,
                )?);
                let results = if let Some(revision) = revision_id {
                    let view = runinator_workspace::storage::view::View::new(
                        objects.as_ref(),
                        revision.parse()?,
                    )?;
                    runinator_workspace::revision::materialize(&view, directory.path())?;
                    runinator_workspace::revision::read_results(&view)?
                } else {
                    Default::default()
                };
                Ok((directory, results, objects))
            })
            .await??;
        Ok(Self {
            execution,
            replica_id,
            path: directory.path().to_owned(),
            directory: Some(directory),
            results,
            objects,
        })
    }
    pub fn results(&self) -> &std::collections::BTreeMap<String, Value> {
        &self.results
    }
    pub fn exposed(&self) -> Result<Value, SendableError> {
        Ok(runinator_models::json!({
            "key": self.execution.key,
            "version": self.execution.checkout.base_version,
            "resolved_path": self.path.as_path().to_string_lossy(),
            "results": self.results,
        }))
    }
    pub async fn save(
        &self,
        api: &AsyncApiClient<StaticLocator>,
        output: Option<&Value>,
    ) -> Result<Option<WorkspaceCommit>, SendableError> {
        if self.execution.checkout.access == WorkspaceAccess::Read {
            return Ok(None);
        }
        let mut results = self.results.clone();
        let output = output.cloned().unwrap_or_default();
        results.insert("result".into(), output.clone());
        for (name, mapping) in &self.execution.results {
            let value = if let Some(pointer) = mapping.get("$output").and_then(Value::as_str) {
                let json: serde_json::Value = output.clone().into();
                json.pointer(pointer)
                    .cloned()
                    .map(Value::from)
                    .ok_or_else(|| {
                        WORKSPACE_INVALID
                            .error(format!("result mapping '{name}' did not match output"))
                    })?
            } else {
                mapping.clone()
            };
            results.insert(name.clone(), value);
        }
        let root = self.path.as_path().to_owned();
        let objects = self.objects.clone();
        let limits = self.execution.checkout.limits;
        let parent = self
            .execution
            .snapshot
            .as_ref()
            .map(|snapshot| snapshot.revision_id.clone());
        let revision_id = tokio::task::spawn_blocking(move || -> Result<_, SendableError> {
            use runinator_workspace::storage::{packs, staging::Staging};
            let scratch = tempfile::tempdir()?;
            let stage = super::workspace_objects::CheckedStore {
                inner: Staging::new(objects.as_ref(), scratch.path())?,
                deadline: objects.clone(),
            };
            let parent = parent.map(|value| value.parse()).transpose()?;
            let (edit, _) = runinator_workspace::revision::capture_from(
                stage,
                parent,
                &root,
                &results,
                limits,
                scratch.path(),
            )?;
            let revision = edit.finish("provider workspace", parent)?;
            packs::seal(
                &edit.store,
                &objects.cached(),
                revision,
                scratch.path(),
                |pack| objects.upload(std::fs::read(pack.path)?),
            )?;
            Ok(revision.to_string())
        })
        .await??;
        let remaining = self.objects.remaining()?;
        let receipt = tokio::time::timeout(
            remaining,
            api.seal_workspace(
                self.execution.checkout.id,
                self.replica_id,
                revision_id,
                remaining,
            ),
        )
        .await??;
        Ok(Some(WorkspaceCommit {
            checkout: receipt.checkout,
            snapshot: receipt.snapshot,
            receipt_id: receipt.id,
        }))
    }
    pub async fn rebind_cached_commit(
        &self,
        api: &AsyncApiClient<StaticLocator>,
        commit: WorkspaceCommit,
    ) -> Result<WorkspaceCommit, SendableError> {
        if commit.snapshot.workspace_id != self.execution.checkout.workspace_id
            || commit.snapshot.parent_version != self.execution.checkout.base_version
        {
            return Err(WORKSPACE_INVALID.error("cached workspace snapshot has a different base"));
        }
        let remaining = self.objects.remaining()?;
        let receipt = tokio::time::timeout(
            remaining,
            api.seal_workspace(
                self.execution.checkout.id,
                self.replica_id,
                commit.snapshot.revision_id,
                remaining,
            ),
        )
        .await??;
        Ok(WorkspaceCommit {
            checkout: receipt.checkout,
            snapshot: receipt.snapshot,
            receipt_id: receipt.id,
        })
    }
    pub fn reference(&self, commit: Option<&WorkspaceCommit>) -> Value {
        runinator_models::json!({"key": self.execution.key, "version": commit.map_or(self.execution.checkout.base_version, |commit| commit.snapshot.version)})
    }
}

impl Drop for ActiveWorkspace {
    fn drop(&mut self) {
        if let Some(directory) = self.directory.take() {
            if let Ok(runtime) = tokio::runtime::Handle::try_current() {
                runtime.spawn_blocking(move || drop(directory));
            } else {
                drop(directory);
            }
        }
    }
}

fn cache_root() -> Result<std::path::PathBuf, SendableError> {
    runinator_platform::app_data::app_data_path("portable-workspaces")
}

pub async fn cleanup_expired() {
    let result = tokio::task::spawn_blocking(|| -> Result<(), SendableError> {
        let root = cache_root()?;
        if !root.exists() {
            return Ok(());
        }
        for entry in std::fs::read_dir(root)? {
            let entry = entry?;
            let name = entry.file_name();
            let Some(expires) = name
                .to_str()
                .and_then(|name| name.strip_prefix("lease-"))
                .and_then(|name| name.split('-').next())
                .and_then(|value| value.parse::<i64>().ok())
            else {
                continue;
            };
            if expires < chrono::Utc::now().timestamp() && entry.file_type()?.is_dir() {
                std::fs::remove_dir_all(entry.path())?;
            }
        }
        Ok(())
    })
    .await;
    if let Err(error) = result.unwrap_or_else(|error| Err(error.into())) {
        tracing::warn!(%error, "failed to remove expired workspace working copies");
    }
}
