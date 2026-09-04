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
}

impl ActiveWorkspace {
    pub async fn restore(
        api: &AsyncApiClient<StaticLocator>,
        value: &Value,
        replica_id: uuid::Uuid,
    ) -> Result<Self, SendableError> {
        let execution: WorkspaceExecution = value.decode()?;
        let bytes = Some({
            let mut retries = 0;
            loop {
                match api
                    .download_workspace_checkout(execution.checkout.id, replica_id)
                    .await
                {
                    Ok(bytes) => break bytes,
                    Err(_) if retries < 20 => {
                        retries += 1;
                        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
                    }
                    Err(error) => return Err(error.into()),
                }
            }
        });
        let digest = execution
            .snapshot
            .as_ref()
            .map(|snapshot| snapshot.archive_sha256.clone());
        let expires = execution.checkout.leased_until.timestamp();
        let (directory, results) =
            tokio::task::spawn_blocking(move || -> Result<_, SendableError> {
                let root = cache_root()?;
                std::fs::create_dir_all(&root)?;
                let directory = tempfile::Builder::new()
                    .prefix(&format!("lease-{expires}-"))
                    .tempdir_in(root)?;
                let results = if let (Some(bytes), Some(digest)) = (bytes, digest) {
                    runinator_workspace::unpack(&bytes, directory.path(), &digest)?
                } else {
                    std::collections::BTreeMap::new()
                };
                Ok((directory, results))
            })
            .await??;
        Ok(Self {
            execution,
            replica_id,
            path: directory.path().to_owned(),
            directory: Some(directory),
            results,
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
        let packed =
            tokio::task::spawn_blocking(move || runinator_workspace::pack(&root, &results))
                .await??;
        let snapshot = api
            .upload_workspace_snapshot(self.execution.checkout.id, self.replica_id, packed.bytes)
            .await?;
        Ok(Some(WorkspaceCommit {
            checkout: self.execution.checkout.clone(),
            snapshot,
        }))
    }
    pub fn rebind_cached_commit(
        &self,
        mut commit: WorkspaceCommit,
    ) -> Result<WorkspaceCommit, SendableError> {
        if commit.snapshot.workspace_id != self.execution.checkout.workspace_id
            || commit.snapshot.parent_version != self.execution.checkout.base_version
        {
            return Err(WORKSPACE_INVALID.error("cached workspace snapshot has a different base"));
        }
        commit.checkout = self.execution.checkout.clone();
        commit.snapshot.attempt = self.execution.checkout.attempt;
        Ok(commit)
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
