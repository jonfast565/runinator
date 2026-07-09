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
    register_replica_session, spawn_replica_heartbeat_with_telemetry,
};
use runinator_comm::{ConsumerProfile, ControlKind};
use runinator_models::replicas::ReplicaKind;
use runinator_plugin::provider::Provider;
use runinator_provider_catalog::{StaticProvider, built_in_providers};
use runinator_provider_local_files::LocalProvider;
use runinator_utilities::resource_telemetry::{TelemetryCollector, attributes_with_host_metadata};
use runinator_worker::{
    ActionOutcome, ProviderFactory, WorkerEvent, WorkerEventSink, WorkerRuntime, parse_labels,
    start_worker_loop,
};
use tokio::sync::Notify;
use uuid::Uuid;

pub use crate::config::AgentConfig;

// the pool label that marks this replica (and the actions targeted to it) as desktop work.
const POOL_LABEL: &str = "desktop";
// env vars the local-files provider reads at execution time; set in-process before the loop starts.
const ROOT_ENV: &str = "RUNINATOR_LOCAL_FILES_ROOT";
const ALLOW_WRITE_ENV: &str = "RUNINATOR_LOCAL_FILES_ALLOW_WRITE";
// rolling cap on retained console lines; the oldest are dropped once it fills, so the buffer never
// grows without bound during a long-running session.
const MAX_LOG_LINES: usize = 10_000;
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

/// where the worker loop is in the connect/retry cycle. surfaced in the header and tray so a degraded
/// agent (broker unreachable, loop crash-looping) is visible at a glance without opening the log.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub enum ConnectionState {
    /// no worker loop running (agent stopped or never started).
    #[default]
    Stopped,
    /// building the broker connection / bringing the loop up.
    Connecting,
    /// the worker loop is up and consuming actions.
    Connected,
    /// the loop exited or the broker failed; backing off before the next attempt.
    Reconnecting { retry_secs: u64 },
}

/// a single finished action, kept so the header can show what this machine last did.
#[derive(Debug, Clone)]
pub struct CompletedAction {
    pub summary: String,
    pub outcome: ActionOutcome,
    pub duration_ms: i64,
}

/// live worker-loop counters and the latest resource sample, surfaced in the status header. updated
/// from the worker event sink and the telemetry sampler; reset on each start.
#[derive(Debug, Clone, Default)]
pub struct AgentMetrics {
    pub in_flight: u32,
    pub succeeded: u64,
    pub failed: u64,
    pub timed_out: u64,
    pub canceled: u64,
    pub skipped_duplicates: u64,
    pub last_completed: Option<CompletedAction>,
    pub cpu_percent: Option<f32>,
    pub mem_percent: Option<f32>,
}

/// state shared between the GUI thread and the background tokio runtime driving the agent.
#[derive(Default)]
pub struct Shared {
    pub status: AgentStatus,
    pub connection: ConnectionState,
    pub metrics: AgentMetrics,
    pub busy: bool,
    pub logs: VecDeque<String>,
    // latch so one degraded episode fires exactly one "disconnected" toast (and one "reconnected"
    // toast on recovery), rather than one per backoff retry.
    degraded_notified: bool,
    shutdown: Option<Arc<Notify>>,
    handle: Option<tokio::task::JoinHandle<()>>,
}

pub type SharedHandle = Arc<Mutex<Shared>>;

pub(crate) fn log_line(shared: &SharedHandle, line: impl Into<String>) {
    let mut guard = shared.lock().expect("desktop agent state lock poisoned");
    push_log_line(&mut guard, line);
}

/// non-blocking variant for the tracing bridge (`crate::logging`): a tracing event can fire while
/// another path holds the state lock, and a blocking lock there would deadlock the emitting thread,
/// so drop the line under contention rather than block.
pub(crate) fn try_log_line(shared: &SharedHandle, line: impl Into<String>) {
    if let Ok(mut guard) = shared.try_lock() {
        push_log_line(&mut guard, line);
    }
}

// record the current connect/retry phase for the header/tray, and fire a native toast when a degraded
// episode begins or ends. cheap enough to call on every transition since it only touches the shared
// state under a short lock (the toast itself is dispatched off-thread by `crate::notify`).
fn set_connection(shared: &SharedHandle, state: ConnectionState) {
    // decide the notification under the lock (so the latch is race-free), but fire it after
    // releasing — `notify` only spawns a thread, yet keeping platform calls off a held lock is the
    // habit worth keeping.
    let toast = {
        let mut guard = shared.lock().expect("desktop agent state lock poisoned");
        guard.connection = state.clone();
        match &state {
            ConnectionState::Reconnecting { .. } if !guard.degraded_notified => {
                guard.degraded_notified = true;
                Some(Toast::Degraded)
            }
            ConnectionState::Connected if guard.degraded_notified => {
                guard.degraded_notified = false;
                Some(Toast::Recovered)
            }
            _ => None,
        }
    };
    match toast {
        Some(Toast::Degraded) => crate::notify::notify_degraded("The broker is unreachable."),
        Some(Toast::Recovered) => crate::notify::notify_recovered(),
        None => {}
    }
}

// which health toast a connection transition warrants, if any.
enum Toast {
    Degraded,
    Recovered,
}

// fold one worker-loop event into the running counters so the header reflects throughput without
// the operator parsing log lines. failures never panic the sink: a poisoned lock just drops the
// update.
fn apply_event_metrics(shared: &SharedHandle, event: &WorkerEvent) {
    let Ok(mut guard) = shared.lock() else {
        return;
    };
    match event {
        WorkerEvent::ActionStarted { .. } => {
            guard.metrics.in_flight = guard.metrics.in_flight.saturating_add(1);
        }
        WorkerEvent::ActionSkippedDuplicate { .. } => {
            guard.metrics.skipped_duplicates = guard.metrics.skipped_duplicates.saturating_add(1);
        }
        WorkerEvent::ActionFinished {
            provider,
            function,
            node_id,
            outcome,
            duration_ms,
            ..
        } => {
            guard.metrics.in_flight = guard.metrics.in_flight.saturating_sub(1);
            match outcome {
                ActionOutcome::Succeeded => guard.metrics.succeeded += 1,
                ActionOutcome::Failed => guard.metrics.failed += 1,
                ActionOutcome::TimedOut => guard.metrics.timed_out += 1,
                ActionOutcome::Canceled => guard.metrics.canceled += 1,
            }
            guard.metrics.last_completed = Some(CompletedAction {
                summary: format!("{provider}.{function} ({node_id})"),
                outcome: *outcome,
                duration_ms: *duration_ms,
            });
        }
        WorkerEvent::ControlReceived { .. } => {}
    }
}

fn push_log_line(shared: &mut Shared, line: impl Into<String>) {
    if shared.logs.len() >= MAX_LOG_LINES {
        shared.logs.pop_front();
    }
    let stamped = format!(
        "{} {}",
        chrono::Local::now().format("%H:%M:%S"),
        line.into()
    );
    shared.logs.push_back(stamped);
}

// first uuid segment; enough to correlate console lines with the run in the command center.
fn short_id(id: &Uuid) -> String {
    id.to_string().chars().take(8).collect()
}

/// render a worker-loop event as one console line, so the operator can see what this machine is
/// processing rather than only that the loop started.
fn describe_worker_event(event: &WorkerEvent) -> String {
    match event {
        WorkerEvent::ActionStarted {
            workflow_run_id,
            node_id,
            provider,
            function,
            attempt,
            ..
        } => {
            let attempt_suffix = if *attempt > 1 {
                format!(", attempt {attempt}")
            } else {
                String::new()
            };
            format!(
                "Executing {provider}.{function} (node '{node_id}', run {}{attempt_suffix})...",
                short_id(workflow_run_id)
            )
        }
        WorkerEvent::ActionSkippedDuplicate { node_run_id } => format!(
            "Skipped duplicate delivery for node run {}: another worker holds it.",
            short_id(node_run_id)
        ),
        WorkerEvent::ActionFinished {
            workflow_run_id,
            node_id,
            provider,
            function,
            outcome,
            duration_ms,
            message,
            ..
        } => {
            let subject = format!(
                "{provider}.{function} (node '{node_id}', run {})",
                short_id(workflow_run_id)
            );
            match outcome {
                ActionOutcome::Succeeded => {
                    format!("Completed {subject} in {duration_ms} ms.")
                }
                ActionOutcome::TimedOut => format!("Timed out {subject} after {duration_ms} ms."),
                ActionOutcome::Canceled => format!("Canceled {subject} after {duration_ms} ms."),
                ActionOutcome::Failed => format!(
                    "Failed {subject} after {duration_ms} ms: {}.",
                    message.as_deref().unwrap_or("no error message")
                ),
            }
        }
        WorkerEvent::ControlReceived {
            kind,
            workflow_run_id,
        } => {
            let kind = match kind {
                ControlKind::Cancel => "cancel",
                ControlKind::Pause => "pause",
                ControlKind::Resume => "resume",
            };
            format!(
                "Received {kind} control for run {}.",
                short_id(workflow_run_id)
            )
        }
    }
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
        guard.connection = ConnectionState::Stopped;
        guard.metrics = AgentMetrics::default();
        guard.degraded_notified = false;
        guard.busy = false;
    });
}

async fn run_agent(shared: &SharedHandle, config: AgentConfig) -> Result<(), String> {
    {
        // start from a clean slate so counters/telemetry reflect this run, not the previous one.
        let mut guard = shared.lock().expect("desktop agent state lock poisoned");
        guard.metrics = AgentMetrics::default();
        guard.connection = ConnectionState::Connecting;
        guard.degraded_notified = false;
    }
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
            attributes: attributes_with_host_metadata(&runinator_models::json!({
                "pool": POOL_LABEL,
                "exclusive": true,
                "labels": labels,
            })),
            heartbeat_interval: Duration::from_secs(10),
        },
    )
    .await
    .map_err(|err| err.to_string())?;
    let replica_id = session.replica_id();
    log_line(shared, format!("Registered replica {replica_id}."));
    // surface the advertised labels so the operator can confirm this machine is opted into the packs
    // that pin to it (e.g. `runner=creds-sync`); a label-targeted action only routes here when these
    // satisfy its selector.
    let labels_display = labels
        .iter()
        .map(|(key, value)| format!("{key}={value}"))
        .collect::<Vec<_>>()
        .join(", ");
    log_line(shared, format!("Advertising labels: {labels_display}"));

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
        // base directory console commands run from, so a workflow can reference files by a relative
        // path from a repo checkout (e.g. `packs/creds-sync`'s `bash scripts/sync-secrets.sh`) rather
        // than an absolute path baked in at import. empty leaves the console provider on the agent's cwd.
        if config.console_working_dir.trim().is_empty() {
            std::env::remove_var(runinator_provider_console::WORKING_DIR_ENV);
        } else {
            std::env::set_var(
                runinator_provider_console::WORKING_DIR_ENV,
                config.console_working_dir.trim(),
            );
        }
        // this worker runs in the operator's desktop session, so `console.run(interactive: true)` can
        // attach to a real terminal (browser login, Keychain dialog). a headless cloud worker never
        // sets this, so the console provider rejects interactive commands there instead of hanging.
        std::env::set_var(runinator_provider_console::ALLOW_INTERACTIVE_ENV, "1");
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
    // heartbeat keeps the replica Live, marks it offline on shutdown, and samples cpu/ram/gpu on
    // every tick so this replica shows up the same as a standalone cloud worker in the statistics view.
    let telemetry = Arc::new(TelemetryCollector::new());
    spawn_replica_heartbeat_with_telemetry(
        api_client.clone(),
        session,
        shutdown.clone(),
        Some(telemetry.clone()),
    );
    spawn_telemetry_sampler(shared.clone(), telemetry, shutdown.clone());

    let shared_loop = shared.clone();
    let shutdown_loop = shutdown.clone();
    let max_concurrent_actions = config.max_concurrent_actions.max(1);
    let shutdown_grace = Duration::from_secs(config.shutdown_grace_seconds.max(1));
    let handle = tokio::spawn(async move {
        run_worker_loop_with_restart(
            &shared_loop,
            broker_config,
            profile,
            api_client,
            replica_id,
            providers,
            shutdown_loop,
            max_concurrent_actions,
            shutdown_grace,
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
    max_concurrent_actions: usize,
    shutdown_grace: Duration,
) {
    let mut retry_delay = WORKER_LOOP_RETRY_BASE;
    // bridge worker-loop activity into the status console; one sink shared by every restart.
    let shared_events = shared.clone();
    let events: Arc<dyn WorkerEventSink> = Arc::new(move |event: WorkerEvent| {
        apply_event_metrics(&shared_events, &event);
        log_line(&shared_events, describe_worker_event(&event));
    });

    loop {
        set_connection(shared, ConnectionState::Connecting);
        let broker = match runinator_worker::build_broker(&broker_config).await {
            Ok(broker) => broker,
            Err(err) => {
                set_connection(
                    shared,
                    ConnectionState::Reconnecting {
                        retry_secs: retry_delay.as_secs(),
                    },
                );
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
            max_concurrent_actions,
            shutdown_grace,
            shutdown: shutdown.clone(),
            events: events.clone(),
        };

        set_connection(shared, ConnectionState::Connected);
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

        set_connection(
            shared,
            ConnectionState::Reconnecting {
                retry_secs: retry_delay.as_secs(),
            },
        );
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

// cadence for refreshing the header's cpu/ram readout; the heartbeat already reports telemetry to the
// service, this is only the local mirror for the status window.
const TELEMETRY_SAMPLE_INTERVAL: Duration = Duration::from_secs(3);

/// periodically sample host cpu/memory into `shared` for the status header, until `shutdown` fires.
/// the sample runs on a blocking thread since it refreshes system counters; kept separate from the
/// heartbeat so the window updates even between heartbeat ticks.
fn spawn_telemetry_sampler(
    shared: SharedHandle,
    telemetry: Arc<TelemetryCollector>,
    shutdown: Arc<Notify>,
) {
    tokio::spawn(async move {
        loop {
            let collector = telemetry.clone();
            if let Ok(sample) = tokio::task::spawn_blocking(move || collector.sample()).await
                && let Ok(mut guard) = shared.lock()
            {
                guard.metrics.cpu_percent = Some(sample.cpu_percent);
                guard.metrics.mem_percent = Some(sample.mem_percent);
            }
            if wait_or_shutdown(&shutdown, TELEMETRY_SAMPLE_INTERVAL).await {
                return;
            }
        }
    });
}

/// one-shot connectivity check for the GUI's "Test connection" button: builds a throwaway client
/// from the given url/key and lists worker replicas, logging the outcome. never touches the running
/// agent, so it is safe to run whether started or stopped.
pub fn test_connection(
    rt: &tokio::runtime::Handle,
    shared: SharedHandle,
    service_url: String,
    api_key: Option<String>,
) {
    rt.spawn(async move {
        log_line(&shared, format!("Testing connection to {service_url} ..."));
        let client =
            match AsyncApiClient::with_credentials(StaticLocator::new(service_url), api_key) {
                Ok(client) => client,
                Err(err) => {
                    log_line(&shared, format!("Connection test failed: {err}"));
                    return;
                }
            };
        match client.fetch_replicas(Some(ReplicaKind::Worker), None).await {
            Ok(list) => log_line(
                &shared,
                format!(
                    "Connection OK: service reachable, {} worker replica(s) registered.",
                    list.replicas.len()
                ),
            ),
            Err(err) => log_line(&shared, format!("Connection test failed: {err}")),
        }
    });
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
