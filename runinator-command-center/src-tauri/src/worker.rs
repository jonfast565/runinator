//! the optional in-process desktop worker. when started, the command center registers itself as an
//! exclusive `desktop` worker replica, publishes the local-files provider, and runs the shared worker
//! action loop against the broker — so workflows can target this machine's files. exclusive +
//! replica-bound, so it only ever receives actions the reducer pins to this exact replica.

use std::collections::{BTreeMap, HashMap};
use std::sync::Arc;
use std::time::Duration;

use runinator_api::{
    register_replica_provider, register_replica_session, spawn_replica_heartbeat, AsyncApiClient,
    ReplicaServiceConfig, StaticLocator,
};
use runinator_broker::http::client::HttpBroker;
use runinator_comm::ConsumerProfile;
use runinator_models::replicas::ReplicaKind;
use runinator_plugin::provider::Provider;
use runinator_provider_catalog::StaticProvider;
use runinator_provider_local_files::LocalProvider;
use runinator_worker::{start_worker_loop, ProviderFactory, WorkerRuntime};
use serde::{Deserialize, Serialize};
use tauri::State;
use tokio::sync::Notify;
use uuid::Uuid;

use crate::error::{CommandError, CommandResult};
use crate::state::CommandCenterState;

// the pool label that marks this replica (and the actions targeted to it) as desktop work.
const POOL_LABEL: &str = "desktop";
// env vars the local-files provider reads at execution time; set in-process before the loop starts.
const ROOT_ENV: &str = "RUNINATOR_LOCAL_FILES_ROOT";
const ALLOW_WRITE_ENV: &str = "RUNINATOR_LOCAL_FILES_ALLOW_WRITE";

/// lifecycle handle for the in-process worker. `default()` is the not-running state.
#[derive(Default)]
pub struct EmbeddedWorker {
    pub replica_id: Option<Uuid>,
    pub root: Option<String>,
    pub broker_url: Option<String>,
    shutdown: Option<Arc<Notify>>,
    handle: Option<tokio::task::JoinHandle<()>>,
}

impl EmbeddedWorker {
    fn running(&self) -> bool {
        self.handle.is_some()
    }
}

/// start request from the desktop settings UI.
#[derive(Debug, Deserialize)]
pub struct LocalWorkerConfig {
    pub broker_url: String,
    pub sandbox_root: String,
    #[serde(default)]
    pub allow_write: bool,
    #[serde(default)]
    pub user_id: Option<String>,
}

/// status reported back to the UI.
#[derive(Debug, Serialize)]
pub struct LocalWorkerStatus {
    pub running: bool,
    pub replica_id: Option<String>,
    pub root: Option<String>,
    pub broker_url: Option<String>,
}

#[tauri::command]
pub async fn local_worker_status(
    state: State<'_, CommandCenterState>,
) -> CommandResult<LocalWorkerStatus> {
    let worker = state.embedded_worker.read().await;
    Ok(status_of(&worker))
}

#[tauri::command]
pub async fn start_local_worker(
    state: State<'_, CommandCenterState>,
    config: LocalWorkerConfig,
) -> CommandResult<LocalWorkerStatus> {
    if state.embedded_worker.read().await.running() {
        return Ok(status_of(&*state.embedded_worker.read().await));
    }

    let service_url = state
        .service_url
        .read()
        .await
        .clone()
        .ok_or(CommandError::NoService)?;
    let token = state.access_token.read().await.clone();

    let api_client =
        AsyncApiClient::with_credentials(StaticLocator::new(service_url), token.clone())
            .map_err(|err| CommandError::Unexpected(err.to_string()))?;

    // register this desktop as an exclusive worker replica so the reducer can pin local actions here.
    let instance_id = Uuid::new_v4().to_string();
    let session = register_replica_session(
        &api_client,
        ReplicaServiceConfig {
            replica_type: ReplicaKind::Worker,
            instance_id: instance_id.clone(),
            display_name: Some(format!("desktop-{instance_id}")),
            host: None,
            port: None,
            base_path: None,
            version: Some(env!("CARGO_PKG_VERSION").to_string()),
            attributes: runinator_models::json!({
                "pool": POOL_LABEL,
                "exclusive": true,
                "user_id": config.user_id,
            }),
            heartbeat_interval: Duration::from_secs(10),
        },
    )
    .await
    .map_err(|err| CommandError::Unexpected(err.to_string()))?;
    let replica_id = session.replica_id();

    // publish the local-files provider metadata so the service knows this replica can run it.
    register_replica_provider(&api_client, &session, LocalProvider.metadata())
        .await
        .map_err(|err| CommandError::Unexpected(err.to_string()))?;

    // configure the sandbox the local-files provider confines itself to. read at execution time.
    // safety: set before the worker loop spawns provider execution.
    unsafe {
        std::env::set_var(ROOT_ENV, &config.sandbox_root);
        if config.allow_write {
            std::env::set_var(ALLOW_WRITE_ENV, "1");
        } else {
            std::env::remove_var(ALLOW_WRITE_ENV);
        }
    }

    let broker_url = reqwest::Url::parse(&config.broker_url)
        .map_err(|err| CommandError::Unexpected(err.to_string()))?;
    let broker = Arc::new(HttpBroker::new(
        broker_url,
        bearer_client(token.as_deref())?,
    ));

    // an exclusive, replica-bound consumer: never receives general-pool `Any` work, only actions the
    // reducer pinned to this replica.
    let mut labels = BTreeMap::new();
    labels.insert("pool".to_string(), POOL_LABEL.to_string());
    let profile = ConsumerProfile::shared(replica_id.to_string())
        .with_replica_id(replica_id)
        .with_labels(labels)
        .exclusive();

    // the desktop runs only the local-files provider; it never executes general server workloads.
    let providers: ProviderFactory = Arc::new(|| vec![Box::new(LocalProvider) as StaticProvider]);

    let shutdown = Arc::new(Notify::new());
    // heartbeat keeps the replica Live and marks it offline on shutdown.
    spawn_replica_heartbeat(api_client.clone(), session, shutdown.clone());

    let runtime = WorkerRuntime {
        broker,
        profile,
        libraries: Arc::new(HashMap::new()),
        api_client,
        replica_id: Some(replica_id),
        providers,
        max_concurrent_actions: 2,
        shutdown_grace: Duration::from_secs(10),
        shutdown: shutdown.clone(),
    };

    let handle = tokio::spawn(async move {
        if let Err(err) = start_worker_loop(runtime).await {
            eprintln!("embedded desktop worker loop ended: {err}");
        }
    });

    let mut worker = state.embedded_worker.write().await;
    worker.replica_id = Some(replica_id);
    worker.root = Some(config.sandbox_root);
    worker.broker_url = Some(config.broker_url);
    worker.shutdown = Some(shutdown);
    worker.handle = Some(handle);
    Ok(status_of(&worker))
}

#[tauri::command]
pub async fn stop_local_worker(
    state: State<'_, CommandCenterState>,
) -> CommandResult<LocalWorkerStatus> {
    let (shutdown, handle) = {
        let mut worker = state.embedded_worker.write().await;
        (worker.shutdown.take(), worker.handle.take())
    };
    if let Some(shutdown) = shutdown {
        shutdown.notify_waiters();
    }
    if let Some(handle) = handle {
        // best-effort drain; the loop's own grace period bounds in-flight work.
        let _ = tokio::time::timeout(Duration::from_secs(15), handle).await;
    }
    let mut worker = state.embedded_worker.write().await;
    worker.replica_id = None;
    Ok(status_of(&worker))
}

fn status_of(worker: &EmbeddedWorker) -> LocalWorkerStatus {
    LocalWorkerStatus {
        running: worker.running(),
        replica_id: worker.replica_id.map(|id| id.to_string()),
        root: worker.root.clone(),
        broker_url: worker.broker_url.clone(),
    }
}

// a reqwest client carrying the bearer token as a default header (ready for an authenticated broker).
fn bearer_client(token: Option<&str>) -> CommandResult<reqwest::Client> {
    let mut builder = reqwest::Client::builder();
    if let Some(token) = token.filter(|value| !value.is_empty()) {
        if let Ok(value) = reqwest::header::HeaderValue::from_str(&format!("Bearer {token}")) {
            let mut headers = reqwest::header::HeaderMap::new();
            headers.insert(reqwest::header::AUTHORIZATION, value);
            builder = builder.default_headers(headers);
        }
    }
    builder
        .build()
        .map_err(|err| CommandError::Unexpected(err.to_string()))
}
