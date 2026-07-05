//! the agent's worker lifecycle: registers this machine as an exclusive `desktop` worker replica,
//! publishes the local-files provider, and drives the shared `runinator-worker` action loop against
//! the broker so workflows can read and write files in a sandboxed folder on this machine. this is
//! the same runtime the standalone `runinator-worker` binary uses; only the provider set and replica
//! attributes are specialized for desktop use, so it never picks up general-pool workloads.

use std::collections::{BTreeMap, VecDeque};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use runinator_api::{
    AsyncApiClient, ReplicaServiceConfig, StaticLocator, register_replica_provider,
    register_replica_session, spawn_replica_heartbeat,
};
use runinator_broker::http::client::HttpBroker;
use runinator_comm::ConsumerProfile;
use runinator_models::replicas::ReplicaKind;
use runinator_plugin::provider::Provider;
use runinator_provider_catalog::StaticProvider;
use runinator_provider_local_files::LocalProvider;
use runinator_worker::{ProviderFactory, WorkerRuntime, start_worker_loop};
use tokio::sync::Notify;
use uuid::Uuid;

pub use crate::config::AgentConfig;

// the pool label that marks this replica (and the actions targeted to it) as desktop work.
const POOL_LABEL: &str = "desktop";
// env vars the local-files provider reads at execution time; set in-process before the loop starts.
const ROOT_ENV: &str = "RUNINATOR_LOCAL_FILES_ROOT";
const ALLOW_WRITE_ENV: &str = "RUNINATOR_LOCAL_FILES_ALLOW_WRITE";
// cap kept small: this is a status console, not a log viewer.
const MAX_LOG_LINES: usize = 400;

#[derive(Debug, Clone, Default)]
pub struct AgentStatus {
    pub running: bool,
    pub replica_id: Option<Uuid>,
    pub root: Option<String>,
    pub broker_url: Option<String>,
}

/// state shared between the GUI thread and the background tokio runtime driving the agent.
#[derive(Default)]
pub struct Shared {
    pub status: AgentStatus,
    pub busy: bool,
    pub logs: VecDeque<String>,
    shutdown: Option<Arc<Notify>>,
    handle: Option<tokio::task::JoinHandle<()>>,
}

pub type SharedHandle = Arc<Mutex<Shared>>;

fn log_line(shared: &SharedHandle, line: impl Into<String>) {
    let mut guard = shared.lock().expect("desktop agent state lock poisoned");
    if guard.logs.len() >= MAX_LOG_LINES {
        guard.logs.pop_front();
    }
    guard.logs.push_back(line.into());
}

/// kick off registration and the worker loop on `rt`; returns immediately, updating `shared` as
/// startup progresses. a no-op if the agent is already running or mid-transition.
pub fn start(rt: &tokio::runtime::Handle, shared: SharedHandle, config: AgentConfig) {
    {
        let mut guard = shared.lock().expect("desktop agent state lock poisoned");
        if guard.status.running || guard.busy {
            return;
        }
        guard.busy = true;
    }

    let shared_task = shared.clone();
    rt.spawn(async move {
        if let Err(err) = run_agent(&shared_task, config).await {
            log_line(
                &shared_task,
                format!("Failed to start desktop agent: {err}"),
            );
        }
        shared_task
            .lock()
            .expect("desktop agent state lock poisoned")
            .busy = false;
    });
}

/// signal shutdown and drain the worker loop; returns immediately, updating `shared` once stopped.
pub fn stop(rt: &tokio::runtime::Handle, shared: SharedHandle) {
    let (shutdown, handle) = {
        let mut guard = shared.lock().expect("desktop agent state lock poisoned");
        if guard.busy {
            return;
        }
        guard.busy = true;
        (guard.shutdown.take(), guard.handle.take())
    };

    let Some(shutdown) = shutdown else {
        shared
            .lock()
            .expect("desktop agent state lock poisoned")
            .busy = false;
        return;
    };
    shutdown.notify_waiters();

    let shared_task = shared.clone();
    rt.spawn(async move {
        if let Some(handle) = handle {
            // best-effort drain; the loop's own grace period bounds in-flight work.
            let _ = tokio::time::timeout(Duration::from_secs(15), handle).await;
        }
        log_line(&shared_task, "Desktop agent stopped.");
        let mut guard = shared_task
            .lock()
            .expect("desktop agent state lock poisoned");
        guard.status = AgentStatus::default();
        guard.busy = false;
    });
}

async fn run_agent(shared: &SharedHandle, config: AgentConfig) -> Result<(), String> {
    log_line(shared, format!("Connecting to {} ...", config.service_url));

    let api_client = AsyncApiClient::with_credentials(
        StaticLocator::new(config.service_url.clone()),
        config.api_key.clone(),
    )
    .map_err(|err| err.to_string())?;

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
            }),
            heartbeat_interval: Duration::from_secs(10),
        },
    )
    .await
    .map_err(|err| err.to_string())?;
    let replica_id = session.replica_id();
    log_line(shared, format!("Registered replica {replica_id}."));

    // publish the local-files provider metadata so the service knows this replica can run it.
    register_replica_provider(&api_client, &session, LocalProvider.metadata())
        .await
        .map_err(|err| err.to_string())?;
    log_line(shared, "Published local-files provider metadata.");

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

    let broker_url = reqwest::Url::parse(&config.broker_url).map_err(|err| err.to_string())?;
    let broker = Arc::new(HttpBroker::new(
        broker_url,
        bearer_client(config.api_key.as_deref())?,
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
        libraries: Arc::new(std::collections::HashMap::new()),
        api_client,
        replica_id: Some(replica_id),
        providers,
        max_concurrent_actions: 2,
        shutdown_grace: Duration::from_secs(10),
        shutdown: shutdown.clone(),
    };

    let shared_loop = shared.clone();
    let handle = tokio::spawn(async move {
        if let Err(err) = start_worker_loop(runtime).await {
            log_line(
                &shared_loop,
                format!("Worker loop ended with an error: {err}"),
            );
        }
    });

    let mut guard = shared.lock().expect("desktop agent state lock poisoned");
    guard.status = AgentStatus {
        running: true,
        replica_id: Some(replica_id),
        root: Some(config.sandbox_root),
        broker_url: Some(config.broker_url),
    };
    guard.shutdown = Some(shutdown);
    guard.handle = Some(handle);
    drop(guard);
    log_line(shared, "Desktop agent running.");
    Ok(())
}

// a reqwest client carrying the bearer token as a default header (ready for an authenticated broker).
fn bearer_client(token: Option<&str>) -> Result<reqwest::Client, String> {
    let mut builder = reqwest::Client::builder();
    if let Some(token) = token.filter(|value| !value.is_empty())
        && let Ok(value) = reqwest::header::HeaderValue::from_str(&format!("Bearer {token}"))
    {
        let mut headers = reqwest::header::HeaderMap::new();
        headers.insert(reqwest::header::AUTHORIZATION, value);
        builder = builder.default_headers(headers);
    }
    builder.build().map_err(|err| err.to_string())
}
