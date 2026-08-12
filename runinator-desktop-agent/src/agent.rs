//! this machine's worker lifecycle, hosted on the shared runtime in `runinator_worker::agent`.
//!
//! everything about *being* a worker — registering the replica, publishing providers, heartbeating,
//! and supervising the action loop with reconnect — lives in `runinator-worker` and is byte-for-byte
//! the same code the headless `runinator-worker` binary runs. what this module adds is the desktop
//! part: the sandbox/console environment the local-files and console providers read, the
//! `pool=desktop` exclusivity, and an [`AgentObserver`] that renders lifecycle activity into the
//! in-app console, status header, and native toasts.
//!
//! the agent stays `exclusive`: it never picks up unlabeled general-pool `Any` work, only actions
//! explicitly pinned to its replica id (local-files) or targeted at a label it advertises. beyond the
//! always-on `pool=desktop`, the operator can advertise arbitrary extra labels (e.g.
//! `runner=creds-sync`) so a workflow that needs a desktop instance just needs a matching
//! `.runner("...")` — no new agent code per label.
//!
//! how it reaches the broker ([`crate::config::BrokerMode`]) is orthogonal to being a desktop worker:
//! by default it relays through `runinator-ws` (safe when this machine shouldn't reach the broker
//! directly), but an operator on the trusted network can switch to a direct backend instead.

use std::collections::{BTreeMap, VecDeque};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use runinator_api::{AsyncApiClient, StaticLocator};
use runinator_comm::ControlKind;
use runinator_models::errors::SendableError;
use runinator_models::replicas::ReplicaKind;
use runinator_provider_catalog::{StaticProvider, built_in_providers};
use runinator_provider_local_files::LocalProvider;
use runinator_utilities::resource_telemetry::TelemetryCollector;
use runinator_worker::agent::{
    AgentHandle, AgentObserver, AgentRuntime, AgentRuntimeConfig, BrokerSelection,
    DEFAULT_HEARTBEAT_INTERVAL, DEFAULT_REGISTER_MAX_ATTEMPTS,
};
use runinator_worker::{ActionOutcome, ProviderFactory, WorkerEvent, parse_labels};
use uuid::Uuid;

pub use crate::config::AgentConfig;
pub use runinator_worker::agent::{AgentConnection as ConnectionState, AgentMetrics, short_id};

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
// how long to wait for in-flight work to drain when the operator stops the agent.
const STOP_GRACE: Duration = Duration::from_secs(15);

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
    pub connection: ConnectionState,
    pub metrics: AgentMetrics,
    pub busy: bool,
    pub logs: VecDeque<String>,
    // latch so one degraded episode fires exactly one "disconnected" toast (and one "reconnected"
    // toast on recovery), rather than one per backoff retry.
    degraded_notified: bool,
    handle: Option<AgentHandle>,
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

/// bridges the shared lifecycle into the GUI: console lines, status header, running counters, and
/// the degraded/recovered toasts.
struct DesktopObserver {
    shared: SharedHandle,
    /// the sandbox folder, which the shared status has no notion of.
    root: String,
}

impl AgentObserver for DesktopObserver {
    fn on_log(&self, line: &str) {
        log_line(&self.shared, line);
    }

    fn on_status(&self, status: &runinator_worker::AgentStatus) {
        // decide the notification under the lock (so the latch is race-free), but fire it after
        // releasing — `notify` only spawns a thread, yet keeping platform calls off a held lock is
        // the habit worth keeping.
        let toast = {
            let Ok(mut guard) = self.shared.lock() else {
                return;
            };
            guard.connection = status.connection.clone();
            guard.status = AgentStatus {
                running: status.running,
                replica_id: status.replica_id,
                root: Some(self.root.clone()),
                broker_connection: status.broker_connection.clone(),
            };
            match &status.connection {
                ConnectionState::Reconnecting { .. } if !guard.degraded_notified => {
                    guard.degraded_notified = true;
                    Some(Toast::Degraded)
                }
                ConnectionState::ReenrollmentRequired { .. } if !guard.degraded_notified => {
                    guard.degraded_notified = true;
                    Some(Toast::Credential)
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
            Some(Toast::Credential) => crate::notify::notify_degraded(
                "The agent credential was rejected; re-enrollment is required.",
            ),
            None => {}
        }
    }

    fn on_worker_event(&self, event: &WorkerEvent) {
        if let Ok(mut guard) = self.shared.lock() {
            guard.metrics.apply(event);
        }
        log_line(&self.shared, describe_worker_event(event));
    }
}

// which health toast a connection transition warrants, if any.
enum Toast {
    Degraded,
    Recovered,
    Credential,
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

/// the routing labels this replica advertises: always `pool=desktop`, plus whatever `k=v,k=v` extras
/// the operator configured (same syntax as `RUNINATOR_WORKER_LABELS`). an extra label can override
/// `pool` if the operator sets one explicitly.
fn advertised_labels(config: &AgentConfig) -> BTreeMap<String, String> {
    let mut labels = BTreeMap::new();
    labels.insert("pool".to_string(), POOL_LABEL.to_string());
    labels.extend(parse_labels(Some(&config.extra_labels.join(","))));
    labels
}

/// configure the sandbox and console environment the desktop-only providers read at execution time.
///
/// safety: called before the agent spawns any provider execution.
pub(crate) fn configure_provider_environment(config: &AgentConfig) {
    unsafe {
        std::env::set_var(ROOT_ENV, &config.sandbox_root);
        if config.allow_write {
            std::env::set_var(ALLOW_WRITE_ENV, "1");
        } else {
            std::env::remove_var(ALLOW_WRITE_ENV);
        }
        // base directory console commands run from, so a workflow can reference files by a relative
        // path from a repo checkout (e.g. `packs/creds-sync`'s `bash scripts/sync-secrets.sh`)
        // rather than an absolute path baked in at import. empty leaves the console provider on the
        // agent's own cwd.
        if config.console_working_dir.trim().is_empty() {
            std::env::remove_var(runinator_provider_console::WORKING_DIR_ENV);
        } else {
            std::env::set_var(
                runinator_provider_console::WORKING_DIR_ENV,
                config.console_working_dir.trim(),
            );
        }
        // this worker runs in the operator's desktop session, so `console.run(interactive: true)`
        // can attach to a real terminal (browser login, Keychain dialog). a headless cloud worker
        // never sets this, so the console provider rejects interactive commands there instead of
        // hanging.
        std::env::set_var(runinator_provider_console::ALLOW_INTERACTIVE_ENV, "1");
    }
}

/// build the shared runtime config for this machine. shared by the GUI and headless entry points, so
/// `--headless` cannot drift into configuring a different agent than the tray does.
pub fn runtime_config(config: &AgentConfig) -> Result<AgentRuntimeConfig, SendableError> {
    let labels = advertised_labels(config);
    let (broker, broker_description) = BrokerSelection {
        mode: config.broker_mode.into(),
        service_url: config.service_url.clone(),
        direct_backend: config.direct_broker_backend.clone(),
        direct_endpoint: config.direct_broker_endpoint.clone(),
        action_topic: DEFAULT_ACTION_TOPIC.to_string(),
        control_topic: DEFAULT_CONTROL_TOPIC.to_string(),
        result_topic: DEFAULT_RESULT_TOPIC.to_string(),
        client_id: DEFAULT_BROKER_CLIENT_ID.to_string(),
        api_key: config.api_key.clone(),
    }
    .resolve()?;

    // the full built-in catalog plus the desktop-only local-files provider. safe to always include:
    // `exclusive` below means none of it runs unless a workflow explicitly labels or pins it here.
    let providers: ProviderFactory = Arc::new(|| {
        let mut providers: Vec<StaticProvider> = built_in_providers();
        providers.push(Box::new(LocalProvider));
        providers
    });

    let instance_id = Uuid::new_v4().to_string();
    Ok(AgentRuntimeConfig {
        service_url: config.service_url.clone(),
        api_key: config.api_key.clone(),
        display_name: Some(format!("desktop-{instance_id}")),
        instance_id,
        advertise_host: None,
        version: Some(env!("CARGO_PKG_VERSION").to_string()),
        labels,
        exclusive: true,
        // an exclusive, replica-bound consumer: the replica id is the consumer id, so nothing is
        // shared with the general worker pool.
        consumer_id: None,
        attributes: runinator_models::json!({ "pool": POOL_LABEL }),
        broker,
        broker_description,
        providers,
        // unlike a cloud worker, this machine's provider set is not what the cluster already knows
        // about: it carries local-files, and its catalog is the operator's build. publish it.
        publish_providers: true,
        dll_paths: Vec::new(),
        max_concurrent_actions: config.max_concurrent_actions.max(1),
        shutdown_grace: Duration::from_secs(config.shutdown_grace_seconds.max(1)),
        liveness_file: config.liveness_file.clone(),
        heartbeat_interval: DEFAULT_HEARTBEAT_INTERVAL,
        register_max_attempts: DEFAULT_REGISTER_MAX_ATTEMPTS,
        sample_telemetry: true,
    })
}

/// kick off the agent on `rt`; returns immediately, updating `shared` as startup progresses. a no-op
/// if the agent is already running or mid-transition.
pub fn start(rt: &tokio::runtime::Handle, shared: SharedHandle, config: AgentConfig) {
    {
        let mut guard = shared.lock().expect("desktop agent state lock poisoned");
        if guard.status.running || guard.handle.is_some() || guard.busy {
            return;
        }
        guard.busy = true;
        guard.metrics = AgentMetrics::default();
        guard.connection = ConnectionState::Registering;
        guard.degraded_notified = false;
    }

    configure_provider_environment(&config);
    let observer = Arc::new(DesktopObserver {
        shared: shared.clone(),
        root: config.sandbox_root.clone(),
    });

    let started = runtime_config(&config).and_then(|runtime_config| {
        // `AgentRuntime::start` spawns the lifecycle, so it needs a runtime context; it does not
        // block, so calling it from the GUI thread is fine.
        let _guard = rt.enter();
        AgentRuntime::start(runtime_config, observer)
    });

    let mut guard = shared.lock().expect("desktop agent state lock poisoned");
    guard.busy = false;
    match started {
        Ok(handle) => {
            if let Some(telemetry) = handle.telemetry() {
                spawn_telemetry_sampler(rt, shared.clone(), telemetry, handle.watch());
            }
            guard.handle = Some(handle);
        }
        Err(err) => {
            guard.connection = ConnectionState::Stopped;
            push_log_line(&mut guard, format!("Failed to start desktop agent: {err}"));
        }
    }
}

/// signal shutdown and drain the agent; returns immediately, updating `shared` once stopped.
pub fn stop(rt: &tokio::runtime::Handle, shared: SharedHandle) {
    let handle = {
        let mut guard = shared.lock().expect("desktop agent state lock poisoned");
        if guard.busy {
            return;
        }
        guard.busy = true;
        guard.handle.take()
    };

    let Some(mut handle) = handle else {
        let mut guard = shared.lock().expect("desktop agent state lock poisoned");
        guard.busy = false;
        return;
    };
    // latch the stop before spawning, so an agent still in its registration backoff sees it.
    handle.shutdown();

    let shared_task = shared.clone();
    rt.spawn(async move {
        if let Err(err) = handle.stop(STOP_GRACE).await {
            log_line(&shared_task, format!("Desktop agent stop: {err}"));
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

// cadence for refreshing the header's cpu/ram readout; the heartbeat already reports telemetry to the
// service, this is only the local mirror for the status window.
const TELEMETRY_SAMPLE_INTERVAL: Duration = Duration::from_secs(3);

/// periodically sample host cpu/memory into `shared` for the status header, until the agent stops.
/// the sample runs on a blocking thread since it refreshes system counters; kept separate from the
/// heartbeat so the window updates even between heartbeat ticks.
fn spawn_telemetry_sampler(
    rt: &tokio::runtime::Handle,
    shared: SharedHandle,
    telemetry: Arc<TelemetryCollector>,
    mut status: tokio::sync::watch::Receiver<runinator_worker::AgentStatus>,
) {
    rt.spawn(async move {
        loop {
            let collector = telemetry.clone();
            if let Ok(sample) = tokio::task::spawn_blocking(move || collector.sample()).await
                && let Ok(mut guard) = shared.lock()
            {
                guard.metrics.cpu_percent = Some(sample.cpu_percent);
                guard.metrics.mem_percent = Some(sample.mem_percent);
            }
            // stop sampling when the lifecycle settles, rather than outliving the agent.
            tokio::select! {
                changed = status.changed() => {
                    if changed.is_err()
                        || status.borrow().connection == ConnectionState::Stopped
                    {
                        return;
                    }
                }
                _ = tokio::time::sleep(TELEMETRY_SAMPLE_INTERVAL) => {}
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
