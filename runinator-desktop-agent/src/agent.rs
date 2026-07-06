//! the agent's worker lifecycle: registers this machine as an exclusive `desktop` worker replica,
//! publishes every built-in provider plus the desktop-only local-files provider, and drives the
//! shared `runinator-worker` action loop against the broker. this is the same runtime and the same
//! provider catalog (`runinator_provider_catalog::built_in_providers`) the standalone
//! `runinator-worker` binary uses, so this machine can run anything a cloud worker can — the
//! distinguishing trait is that it stays `exclusive`: it never picks up unlabeled general-pool `Any`
//! work, only actions explicitly pinned to its replica id (local-files) or targeted to a label it
//! advertises. beyond the always-on `pool=desktop`, the operator can advertise arbitrary extra labels
//! (e.g. `runner=creds-sync`) so a future workflow that needs a desktop instance just needs a matching
//! `.runner("...")`/label requirement — no new agent code or GUI control per label.
//!
//! how it reaches the broker ([`crate::config::BrokerMode`]) is a separate, orthogonal choice from
//! being a desktop worker: by default it relays through `runinator-ws` (safe when this machine
//! shouldn't reach the broker directly), but an operator on the trusted network can switch to a
//! direct backend instead — the same `runinator_worker::build_broker` selection the standalone cloud
//! worker binary uses, so either kind of worker can pick either transport.

use std::collections::{BTreeMap, VecDeque};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use runinator_api::{
    AsyncApiClient, ReplicaServiceConfig, StaticLocator, register_replica_provider,
    register_replica_session, spawn_replica_heartbeat,
};
use runinator_comm::ConsumerProfile;
use runinator_models::replicas::ReplicaKind;
use runinator_plugin::provider::Provider;
use runinator_provider_catalog::{StaticProvider, built_in_providers};
use runinator_provider_local_files::LocalProvider;
use runinator_worker::{ProviderFactory, WorkerRuntime, parse_labels, start_worker_loop};
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
// broker channel names/client id; fixed rather than exposed in the GUI — an advanced operator who
// needs to match a non-default cluster naming scheme can still edit the persisted config JSON.
const DEFAULT_ACTION_TOPIC: &str = "runinator.actions";
const DEFAULT_CONTROL_TOPIC: &str = "runinator.control";
const DEFAULT_RESULT_TOPIC: &str = "runinator.results";
const DEFAULT_BROKER_CLIENT_ID: &str = "runinator-desktop-agent";
// backoff for restarting the worker loop after it exits with an error (broker construction failure,
// or a genuine fatal error inside `start_worker_loop`) — grows on repeated failures, capped, and
// resets once a run has stayed up long enough to call it healthy.
const WORKER_LOOP_RETRY_BASE: Duration = Duration::from_secs(2);
const WORKER_LOOP_RETRY_MAX: Duration = Duration::from_secs(60);
const WORKER_LOOP_HEALTHY_AFTER: Duration = Duration::from_secs(60);

#[derive(Debug, Clone, Default)]
pub struct AgentStatus {
    pub running: bool,
    pub replica_id: Option<Uuid>,
    pub root: Option<String>,
    /// e.g. "relay via wss://.../ws/desktop-worker" or "direct tcp @ host:port".
    pub broker_connection: Option<String>,
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

pub(crate) fn log_line(shared: &SharedHandle, line: impl Into<String>) {
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

    // the routing labels this replica advertises: always `pool=desktop`, plus whatever `k=v,k=v`
    // extras the operator configured (same syntax as `RUNINATOR_WORKER_LABELS`) — e.g.
    // `runner=creds-sync` to opt this machine into `packs/creds-sync`. an extra label can override
    // `pool` if the operator sets one explicitly.
    let mut labels = BTreeMap::new();
    labels.insert("pool".to_string(), POOL_LABEL.to_string());
    labels.extend(parse_labels(Some(&config.extra_labels.join(","))));

    // register this desktop as an exclusive worker replica so the reducer can pin local actions here
    // (and, per `labels`, route label-targeted actions here).
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
                "labels": labels,
            }),
            heartbeat_interval: Duration::from_secs(10),
        },
    )
    .await
    .map_err(|err| err.to_string())?;
    let replica_id = session.replica_id();
    log_line(shared, format!("Registered replica {replica_id}."));

    // publish the local-files provider metadata plus the full built-in catalog, so the service knows
    // this replica can run anything a cloud worker can (routing still gated by `exclusive`/labels).
    register_replica_provider(&api_client, &session, LocalProvider.metadata())
        .await
        .map_err(|err| err.to_string())?;
    for provider in built_in_providers() {
        register_replica_provider(&api_client, &session, provider.metadata())
            .await
            .map_err(|err| err.to_string())?;
    }
    log_line(
        shared,
        "Published provider metadata (local-files + built-ins).",
    );

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

    // which broker transport to use is orthogonal to being a "desktop" worker: relay through
    // runinator-ws's own authenticated, already-exposed endpoint by default (safest for a machine
    // that shouldn't reach the broker directly), or connect straight to a broker backend when the
    // operator knows this machine is on the trusted network and wants to skip the relay hop. either
    // way this goes through the exact same `build_broker` path the standalone cloud worker uses.
    let (broker_config, connection_description) = match config.broker_mode {
        crate::config::BrokerMode::Relay => {
            let relay_url = derive_relay_url(&config.service_url)?;
            let description = format!("relay via {relay_url}");
            (
                runinator_worker::BrokerConfig {
                    broker_backend: "ws".to_string(),
                    broker_endpoint: relay_url,
                    broker_action_topic: DEFAULT_ACTION_TOPIC.to_string(),
                    broker_control_topic: DEFAULT_CONTROL_TOPIC.to_string(),
                    broker_result_topic: DEFAULT_RESULT_TOPIC.to_string(),
                    broker_client_id: DEFAULT_BROKER_CLIENT_ID.to_string(),
                    api_key: config.api_key.clone(),
                },
                description,
            )
        }
        crate::config::BrokerMode::Direct => {
            let description = format!(
                "direct {} @ {}",
                config.direct_broker_backend, config.direct_broker_endpoint
            );
            (
                runinator_worker::BrokerConfig {
                    broker_backend: config.direct_broker_backend.clone(),
                    broker_endpoint: config.direct_broker_endpoint.clone(),
                    broker_action_topic: DEFAULT_ACTION_TOPIC.to_string(),
                    broker_control_topic: DEFAULT_CONTROL_TOPIC.to_string(),
                    broker_result_topic: DEFAULT_RESULT_TOPIC.to_string(),
                    broker_client_id: DEFAULT_BROKER_CLIENT_ID.to_string(),
                    api_key: config.api_key.clone(),
                },
                description,
            )
        }
    };
    // an exclusive, replica-bound consumer: never receives general-pool `Any` work, only actions the
    // reducer pinned to this replica (by replica id) or label-targeted to it via `labels`.
    let profile = ConsumerProfile::shared(replica_id.to_string())
        .with_replica_id(replica_id)
        .with_labels(labels)
        .exclusive();

    // the full built-in catalog plus the desktop-only local-files provider. safe to always include:
    // `exclusive` above means none of it runs unless a workflow explicitly labels/pins it here.
    let providers: ProviderFactory = Arc::new(|| {
        let mut providers: Vec<StaticProvider> = built_in_providers();
        providers.push(Box::new(LocalProvider));
        providers
    });

    let shutdown = Arc::new(Notify::new());
    // heartbeat keeps the replica Live and marks it offline on shutdown.
    spawn_replica_heartbeat(api_client.clone(), session, shutdown.clone());

    let shared_loop = shared.clone();
    let shutdown_loop = shutdown.clone();
    let handle = tokio::spawn(async move {
        run_worker_loop_with_restart(
            &shared_loop,
            broker_config,
            profile,
            api_client,
            replica_id,
            providers,
            shutdown_loop,
        )
        .await;
    });

    let mut guard = shared.lock().expect("desktop agent state lock poisoned");
    guard.status = AgentStatus {
        running: true,
        replica_id: Some(replica_id),
        root: Some(config.sandbox_root),
        broker_connection: Some(connection_description),
    };
    guard.shutdown = Some(shutdown);
    guard.handle = Some(handle);
    drop(guard);
    log_line(shared, "Desktop agent running.");
    Ok(())
}

/// drives `start_worker_loop` with restart-on-failure: a fresh broker connection and
/// [`WorkerRuntime`] are (re)built on every attempt, since a broker that failed to construct or that
/// died mid-run won't get better by reusing the same handle. only a graceful shutdown (the loop
/// returning `Ok`, or `shutdown` firing while backing off) ends the retry loop; any other exit is
/// treated as transient and retried with backoff, so a machine nobody is watching keeps trying to
/// rejoin rather than sitting there silently "running" with a dead loop underneath.
#[allow(clippy::too_many_arguments)]
async fn run_worker_loop_with_restart(
    shared: &SharedHandle,
    broker_config: runinator_worker::BrokerConfig,
    profile: ConsumerProfile,
    api_client: AsyncApiClient<StaticLocator>,
    replica_id: Uuid,
    providers: ProviderFactory,
    shutdown: Arc<Notify>,
) {
    let mut retry_delay = WORKER_LOOP_RETRY_BASE;

    loop {
        let broker = match runinator_worker::build_broker(&broker_config).await {
            Ok(broker) => broker,
            Err(err) => {
                log_line(
                    shared,
                    format!(
                        "Worker loop: failed to connect broker ({err}); retrying in {}s",
                        retry_delay.as_secs()
                    ),
                );
                if wait_or_shutdown(&shutdown, retry_delay).await {
                    return;
                }
                retry_delay = (retry_delay * 2).min(WORKER_LOOP_RETRY_MAX);
                continue;
            }
        };

        let runtime = WorkerRuntime {
            broker,
            profile: profile.clone(),
            libraries: Arc::new(std::collections::HashMap::new()),
            api_client: api_client.clone(),
            replica_id: Some(replica_id),
            providers: providers.clone(),
            max_concurrent_actions: 2,
            shutdown_grace: Duration::from_secs(10),
            shutdown: shutdown.clone(),
        };

        let started_at = std::time::Instant::now();
        match start_worker_loop(runtime).await {
            Ok(()) => return, // graceful shutdown requested by `agent::stop`.
            Err(err) => {
                log_line(shared, format!("Worker loop ended with an error: {err}"));
                if started_at.elapsed() >= WORKER_LOOP_HEALTHY_AFTER {
                    retry_delay = WORKER_LOOP_RETRY_BASE;
                }
            }
        }

        log_line(
            shared,
            format!("Restarting worker loop in {}s...", retry_delay.as_secs()),
        );
        if wait_or_shutdown(&shutdown, retry_delay).await {
            return;
        }
        retry_delay = (retry_delay * 2).min(WORKER_LOOP_RETRY_MAX);
    }
}

/// waits out `delay`, or returns early (with `true`) if `shutdown` fires first — so a restart backoff
/// never delays an intentional stop.
async fn wait_or_shutdown(shutdown: &Notify, delay: Duration) -> bool {
    tokio::select! {
        _ = shutdown.notified() => true,
        _ = tokio::time::sleep(delay) => false,
    }
}

/// derive the ws broker relay URL from the service URL: swap the scheme (`http`->`ws`,
/// `https`->`wss`) and point at `/ws/desktop-worker`. keeps the operator down to configuring one
/// URL instead of a separate broker endpoint.
fn derive_relay_url(service_url: &str) -> Result<String, String> {
    let mut url = reqwest::Url::parse(service_url).map_err(|err| err.to_string())?;
    let scheme = match url.scheme() {
        "http" => "ws",
        "https" => "wss",
        other => return Err(format!("unsupported service URL scheme '{other}'")),
    };
    url.set_scheme(scheme)
        .map_err(|_| "failed to set relay URL scheme".to_string())?;
    url.set_path("ws/desktop-worker");
    Ok(url.to_string())
}
