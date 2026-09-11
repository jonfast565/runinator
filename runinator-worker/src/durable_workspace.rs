//! Restore and snapshot isolated portable workspaces around provider execution.
use runinator_api::{
    ApiError,
    capabilities::{WorkspaceCheckoutClient, WorkspaceObjectTransport},
};
use runinator_models::{
    errors::{SendableError, WORKSPACE_INVALID},
    value::Value,
    workspaces::*,
};

const WORKSPACE_CLAIM_RETRY_INITIAL_DELAY: std::time::Duration =
    std::time::Duration::from_millis(25);
const WORKSPACE_CLAIM_RETRY_MAX_DELAY: std::time::Duration = std::time::Duration::from_millis(250);
const WORKSPACE_CLAIM_SYNC_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(5);

#[derive(Clone, Default)]
pub struct WorkspacePhaseReporter {
    events: std::sync::Arc<std::sync::Mutex<Vec<WorkspacePhaseEvent>>>,
}

impl WorkspacePhaseReporter {
    pub fn start(&self, phase: impl Into<String>) -> WorkspacePhaseTimer {
        WorkspacePhaseTimer {
            reporter: self.clone(),
            phase: phase.into(),
            started_at: chrono::Utc::now(),
            started: std::time::Instant::now(),
            recorded: false,
        }
    }

    pub fn drain(&self) -> Vec<WorkspacePhaseEvent> {
        self.events
            .lock()
            .map(|mut events| std::mem::take(&mut *events))
            .unwrap_or_default()
    }

    fn record(&self, event: WorkspacePhaseEvent) {
        if let Ok(mut events) = self.events.lock() {
            events.push(event);
        }
    }
}

pub struct WorkspacePhaseTimer {
    reporter: WorkspacePhaseReporter,
    phase: String,
    started_at: chrono::DateTime<chrono::Utc>,
    started: std::time::Instant,
    recorded: bool,
}

impl WorkspacePhaseTimer {
    pub fn succeeded(mut self, details: Value) {
        self.finish("succeeded", details);
    }

    fn finish(&mut self, status: &str, details: Value) {
        self.recorded = true;
        self.reporter.record(WorkspacePhaseEvent {
            version: 1,
            phase: self.phase.clone(),
            status: status.into(),
            started_at: self.started_at,
            finished_at: chrono::Utc::now(),
            duration_ms: self.started.elapsed().as_millis().min(u128::from(u64::MAX)) as u64,
            details,
        });
    }
}

impl Drop for WorkspacePhaseTimer {
    fn drop(&mut self) {
        if self.recorded {
            return;
        }

        self.finish(
            "failed",
            runinator_models::json!({"message": "phase did not complete"}),
        );
    }
}

pub struct ActiveWorkspace {
    execution: WorkspaceExecution,
    replica_id: uuid::Uuid,
    directory: Option<tempfile::TempDir>,
    path: std::path::PathBuf,
    results: std::collections::BTreeMap<String, Value>,
    objects: std::sync::Arc<super::workspace_objects::WorkerObjects>,
    phases: WorkspacePhaseReporter,
}

impl ActiveWorkspace {
    pub async fn restore(
        api: &(impl WorkspaceCheckoutClient + WorkspaceObjectTransport + Clone + 'static),
        value: &Value,
        replica_id: uuid::Uuid,
        deadline: std::time::Instant,
        phases: WorkspacePhaseReporter,
        materialize_files: bool,
        load_results: bool,
    ) -> Result<Self, SendableError> {
        let execution: WorkspaceExecution = value.decode()?;
        let expires = execution.checkout.leased_until.timestamp();
        let root = cache_root()?;
        let revision_id = execution
            .snapshot
            .as_ref()
            .map(|snapshot| snapshot.revision_id.clone());
        let archive = if revision_id.is_some() && materialize_files {
            let phase = phases.start("workspace.restore.download");
            let bytes = download_workspace_checkout_after_claim(
                api,
                execution.checkout.id,
                replica_id,
                deadline,
            )
            .await?;
            phase.succeeded(runinator_models::json!({"bytes": bytes.len()}));
            Some(bytes)
        } else {
            None
        };
        let api = api.clone();
        let checkout = execution.checkout.id;
        let limits = execution.checkout.limits;
        let revision_for_import = revision_id.clone();
        let import_phase = archive
            .as_ref()
            .map(|_| phases.start("workspace.restore.index"));
        let (local, usage) = tokio::task::spawn_blocking(move || -> Result<_, SendableError> {
            let Some(bytes) = archive else {
                return Ok((None, WorkspaceUsage::default()));
            };
            let scratch = tempfile::tempdir()?;
            let (store, revision, usage) = runinator_workspace::native::import_packed(
                bytes.as_slice(),
                scratch.path(),
                limits,
            )?;
            if Some(revision.to_string()) != revision_for_import {
                return Err(
                    WORKSPACE_INVALID.error("checkout archive contains a different revision")
                );
            }
            if let Some(phase) = import_phase {
                phase.succeeded(runinator_models::json!({
                    "entries": usage.entries,
                    "logical_bytes": usage.logical_bytes,
                    "objects": store.object_count(),
                    "packs": store.pack_count(),
                }));
            }
            Ok((
                Some(super::workspace_objects::LocalObjects::new(store, scratch)),
                usage,
            ))
        })
        .await??;
        let objects = std::sync::Arc::new(super::workspace_objects::WorkerObjects::new(
            api, checkout, replica_id, deadline, local,
        )?);
        let revision_for_materialize = revision_id.clone();
        let materialize_objects = objects.clone();
        let materialize_phase =
            materialize_files.then(|| phases.start("workspace.restore.materialize"));
        let (directory, results) =
            tokio::task::spawn_blocking(move || -> Result<_, SendableError> {
                std::fs::create_dir_all(&root)?;
                let directory = tempfile::Builder::new()
                    .prefix(&format!("lease-{expires}-"))
                    .tempdir_in(root)?;
                let results = if let Some(revision) = revision_for_materialize
                    && (materialize_files || load_results)
                {
                    let view = runinator_workspace::storage::view::View::new(
                        materialize_objects.as_ref(),
                        revision.parse()?,
                    )?;
                    if materialize_files {
                        runinator_workspace::revision::materialize(&view, directory.path())?;
                    }
                    if load_results {
                        runinator_workspace::revision::read_results(&view)?
                    } else {
                        Default::default()
                    }
                } else {
                    Default::default()
                };
                if let Some(phase) = materialize_phase {
                    phase.succeeded(runinator_models::json!({
                        "entries": usage.entries,
                        "logical_bytes": usage.logical_bytes,
                    }));
                }
                Ok((directory, results))
            })
            .await??;
        Ok(Self {
            execution,
            replica_id,
            path: directory.path().to_owned(),
            directory: Some(directory),
            results,
            objects,
            phases,
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
        api: &dyn WorkspaceCheckoutClient,
        output: Option<&Value>,
        result_only: bool,
    ) -> Result<Option<WorkspaceCommit>, SendableError> {
        if self.execution.checkout.access == WorkspaceAccess::Read {
            let phase = self.phases.start("workspace.snapshot.reuse");
            phase.succeeded(runinator_models::json!({
                "version": self.execution.checkout.base_version,
                "reason": "read-only checkout",
            }));
            return Ok(None);
        }
        let mut results = if result_only {
            std::collections::BTreeMap::new()
        } else {
            self.results.clone()
        };
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
        let base_usage = self
            .execution
            .snapshot
            .as_ref()
            .map(|snapshot| snapshot.usage);
        let phases = self.phases.clone();
        let capture_phase = phases.start(if result_only && parent.is_some() {
            "workspace.snapshot.results"
        } else {
            "workspace.snapshot.capture"
        });
        let revision_id = tokio::task::spawn_blocking(move || -> Result<_, SendableError> {
            use runinator_workspace::storage::{packs, staging::Staging};
            let scratch = tempfile::tempdir()?;
            let staging = if objects.has_complete_local_base() {
                Staging::new_deduplicating(objects.as_ref(), scratch.path())?
            } else {
                Staging::new(objects.as_ref(), scratch.path())?
            };
            let stage = super::workspace_objects::CheckedStore {
                inner: staging,
                deadline: objects.clone(),
            };
            let parent = parent.map(|value| value.parse()).transpose()?;
            let (edit, usage) = match (result_only, parent, base_usage) {
                (true, Some(parent), Some(base_usage)) => {
                    runinator_workspace::revision::checkpoint_result_updates(
                        stage,
                        parent,
                        base_usage,
                        &results,
                        limits,
                        scratch.path(),
                    )?
                }
                _ => runinator_workspace::revision::capture_from(
                    stage,
                    parent,
                    &root,
                    &results,
                    limits,
                    scratch.path(),
                )?,
            };
            let revision = edit.finish("provider workspace", parent)?;
            capture_phase.succeeded(runinator_models::json!({
                "entries": usage.entries,
                "logical_bytes": usage.logical_bytes,
                "results_bytes": usage.results_bytes,
            }));
            let pack_phase = phases.start("workspace.snapshot.pack_upload");
            let mut packs_uploaded = 0u64;
            let mut bytes_uploaded = 0u64;
            packs::seal(
                &edit.store,
                &objects.cached(),
                revision,
                scratch.path(),
                |pack| {
                    let bytes = std::fs::read(pack.path)?;
                    bytes_uploaded = bytes_uploaded.saturating_add(bytes.len() as u64);
                    packs_uploaded = packs_uploaded.saturating_add(1);
                    objects.upload(bytes)
                },
            )?;
            pack_phase.succeeded(runinator_models::json!({
                "packs": packs_uploaded,
                "bytes": bytes_uploaded,
            }));
            let cleanup_phase = phases.start("workspace.snapshot.cleanup");
            drop(edit);
            drop(scratch);
            cleanup_phase.succeeded(runinator_models::json!({
                "staging": "single_spool",
            }));
            Ok(revision.to_string())
        })
        .await??;
        let seal_phase = self.phases.start("workspace.snapshot.seal");
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
        seal_phase.succeeded(runinator_models::json!({
            "version": receipt.snapshot.version,
            "revision_id": receipt.snapshot.revision_id,
        }));
        Ok(Some(WorkspaceCommit {
            checkout: receipt.checkout,
            snapshot: receipt.snapshot,
            receipt_id: receipt.id,
        }))
    }
    pub async fn rebind_cached_commit(
        &self,
        api: &dyn WorkspaceCheckoutClient,
        commit: WorkspaceCommit,
    ) -> Result<WorkspaceCommit, SendableError> {
        if commit.snapshot.workspace_id != self.execution.checkout.workspace_id
            || commit.snapshot.parent_version != self.execution.checkout.base_version
        {
            return Err(WORKSPACE_INVALID.error("cached workspace snapshot has a different base"));
        }
        let phase = self.phases.start("workspace.snapshot.rebind");
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
        phase.succeeded(runinator_models::json!({
            "version": receipt.snapshot.version,
            "revision_id": receipt.snapshot.revision_id,
        }));
        Ok(WorkspaceCommit {
            checkout: receipt.checkout,
            snapshot: receipt.snapshot,
            receipt_id: receipt.id,
        })
    }
    pub fn reference(&self, commit: Option<&WorkspaceCommit>) -> Value {
        runinator_models::json!({"key": self.execution.key, "version": commit.map_or(self.execution.checkout.base_version, |commit| commit.snapshot.version)})
    }

    pub fn drain_phases(&self) -> Vec<WorkspacePhaseEvent> {
        self.phases.drain()
    }
}

async fn download_workspace_checkout_after_claim(
    api: &dyn WorkspaceCheckoutClient,
    checkout_id: uuid::Uuid,
    replica_id: uuid::Uuid,
    deadline: std::time::Instant,
) -> Result<Vec<u8>, SendableError> {
    retry_pending_workspace_claim(deadline, |remaining| {
        api.download_workspace_checkout(checkout_id, replica_id, remaining)
    })
    .await
}

async fn retry_pending_workspace_claim<F, Fut>(
    deadline: std::time::Instant,
    mut download: F,
) -> Result<Vec<u8>, SendableError>
where
    F: FnMut(std::time::Duration) -> Fut,
    Fut: std::future::Future<Output = runinator_api::Result<Vec<u8>>>,
{
    let claim_deadline = std::cmp::min(
        deadline,
        std::time::Instant::now() + WORKSPACE_CLAIM_SYNC_TIMEOUT,
    );
    let mut delay = WORKSPACE_CLAIM_RETRY_INITIAL_DELAY;
    loop {
        let remaining = deadline
            .checked_duration_since(std::time::Instant::now())
            .ok_or_else(workspace_attempt_timed_out)?;
        match download(remaining).await {
            Ok(bytes) => return Ok(bytes),
            Err(error)
                if workspace_claim_is_pending(&error)
                    && std::time::Instant::now() < claim_deadline =>
            {
                let wait =
                    delay.min(claim_deadline.saturating_duration_since(std::time::Instant::now()));
                tokio::time::sleep(wait).await;
                delay = delay.saturating_mul(2).min(WORKSPACE_CLAIM_RETRY_MAX_DELAY);
            }
            Err(error) => return Err(error.into()),
        }
    }
}

fn workspace_claim_is_pending(error: &ApiError) -> bool {
    matches!(
        error,
        ApiError::Http {
            status: reqwest::StatusCode::CONFLICT,
            message,
            ..
        } if message.contains("replica has not claimed this active attempt")
    )
}

fn workspace_attempt_timed_out() -> SendableError {
    runinator_workspace::storage::Error::Io(std::io::Error::new(
        std::io::ErrorKind::TimedOut,
        "workspace attempt deadline exceeded",
    ))
    .into()
}

impl Drop for ActiveWorkspace {
    fn drop(&mut self) {
        let Some(directory) = self.directory.take() else {
            return;
        };

        if let Ok(runtime) = tokio::runtime::Handle::try_current() {
            runtime.spawn_blocking(move || drop(directory));
        } else {
            drop(directory);
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

#[cfg(test)]
#[path = "durable_workspace_tests.rs"]
mod durable_workspace_tests;
