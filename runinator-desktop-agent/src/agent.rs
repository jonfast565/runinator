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
use std::future::Future;
use std::path::{Component, Path, PathBuf};
use std::pin::Pin;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use base64::Engine;
use runinator_api::{AsyncApiClient, StaticLocator};
use runinator_comm::{AgentDirectiveKind, ControlKind};
use runinator_models::errors::SendableError;
use runinator_models::replicas::ReplicaKind;
use runinator_provider_catalog::{StaticProvider, built_in_providers};
use runinator_provider_local_files::LocalProvider;
use runinator_utilities::resource_telemetry::TelemetryCollector;
use runinator_worker::agent::{
    AgentHandle, AgentObserver, AgentRuntime, AgentRuntimeConfig, BrokerSelection,
    DEFAULT_HEARTBEAT_INTERVAL, DEFAULT_REGISTER_MAX_ATTEMPTS, LocatorMode,
};
use runinator_worker::agent::{DirectiveHandler, DirectiveResponse};
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
const DEFAULT_CONTROL_TOPIC: &str = "runinator.control";
const DEFAULT_AGENT_TOPIC: &str = "runinator.agent";
const DEFAULT_EFFECT_TOPIC: &str = "runinator.effects";
const DEFAULT_INFRASTRUCTURE_EFFECT_TOPIC: &str = "runinator.effects.infrastructure";
const DEFAULT_EFFECT_RESULT_TOPIC: &str = "runinator.effect-results";
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
    // latch so one degraded episode fires exactly one "reconnecting" toast (and one "reconnected"
    // toast on recovery), rather than one per backoff retry.
    degraded_notified: bool,
    // a separate latch, because giving up is a different event from retrying: an operator who has
    // already seen "reconnecting" still needs to be told the agent stopped.
    disconnected_notified: bool,
    handle: Option<AgentHandle>,
    // set for the window between a Start click and the lifecycle handle existing. `busy` alone
    // cannot say which transition is in flight, and only a start is cancellable.
    starting: bool,
    // the in-flight `start_inner` task, kept so a cancel can abort it mid-registration rather than
    // leaving the operator to wait out a backoff that may never succeed.
    start_task: Option<tokio::task::JoinHandle<()>>,
    // bumped by every start and every cancel. a startup compares it before publishing its handle, so
    // an abort that lands too late (or one issued before the task was even recorded) still cannot
    // leave a lifecycle running that the operator asked to cancel.
    start_generation: u64,
}

/// what the operator can do to the lifecycle right now, derived from the shared state in one place
/// so the window can never offer a Stop for a phase with nothing to stop — or strand a slow startup
/// with no way out.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Control {
    /// nothing is running: offer Start.
    Startable,
    /// a start is in flight and the action loop is not up yet: offer Cancel startup.
    Starting,
    /// the action loop is up: offer Stop.
    Running,
    /// a stop is draining: offer nothing until it settles.
    Stopping,
}

/// which control the current state warrants. see [`Control`].
pub fn control_state(shared: &Shared) -> Control {
    if shared.starting {
        return Control::Starting;
    }
    if shared.busy {
        return Control::Stopping;
    }
    // registration and the first broker connect happen *after* `start` returns its handle, so a live
    // lifecycle that has not reached the loop is still a startup — and still cancellable.
    if shared
        .handle
        .as_ref()
        .is_some_and(|handle| !handle.is_finished())
    {
        return if shared.status.running {
            Control::Running
        } else {
            Control::Starting
        };
    }
    Control::Startable
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

/// desktop-only bounded access to the UI log ring and configured local-files sandbox.
struct DesktopDirectiveHandler {
    shared: SharedHandle,
    root: PathBuf,
}

impl DirectiveHandler for DesktopDirectiveHandler {
    fn handle<'a>(
        &'a self,
        kind: &'a AgentDirectiveKind,
    ) -> Pin<Box<dyn Future<Output = DirectiveResponse> + Send + 'a>> {
        Box::pin(async move {
            match kind {
                AgentDirectiveKind::TailLogs { lines } => {
                    let count = (*lines).min(MAX_LOG_LINES);
                    let Ok(guard) = self.shared.lock() else {
                        return DirectiveResponse::failed("desktop log buffer is unavailable");
                    };
                    let logs = guard
                        .logs
                        .iter()
                        .rev()
                        .take(count)
                        .rev()
                        .cloned()
                        .collect::<Vec<_>>();
                    DirectiveResponse::completed(runinator_models::json!({ "lines": logs }))
                }
                AgentDirectiveKind::ListSandbox { path } => match resolve_sandbox(&self.root, path)
                {
                    Ok(target) => match std::fs::read_dir(target) {
                        Ok(entries) => {
                            let mut names = entries
                                .filter_map(Result::ok)
                                .take(1_000)
                                .map(|entry| entry.file_name().to_string_lossy().into_owned())
                                .collect::<Vec<_>>();
                            names.sort();
                            DirectiveResponse::completed(
                                runinator_models::json!({ "entries": names }),
                            )
                        }
                        Err(err) => DirectiveResponse::failed(err.to_string()),
                    },
                    Err(err) => DirectiveResponse::failed(err),
                },
                AgentDirectiveKind::FetchFile { path, max_bytes } => {
                    let cap = (*max_bytes).min(8 * 1024 * 1024) as usize;
                    match resolve_sandbox(&self.root, path) {
                        Ok(target) => match std::fs::read(target) {
                            Ok(bytes) if bytes.len() <= cap => {
                                DirectiveResponse::completed(runinator_models::json!({
                                    "size": bytes.len(),
                                    "encoding": "base64",
                                    "content": base64::engine::general_purpose::STANDARD.encode(bytes),
                                }))
                            }
                            Ok(bytes) => DirectiveResponse::failed(format!(
                                "file is {} bytes, above the {} byte limit",
                                bytes.len(),
                                cap
                            )),
                            Err(err) => DirectiveResponse::failed(err.to_string()),
                        },
                        Err(err) => DirectiveResponse::failed(err),
                    }
                }
                _ => DirectiveResponse::unsupported("directive is not desktop-specific"),
            }
        })
    }
}

fn resolve_sandbox(root: &Path, relative: &str) -> Result<PathBuf, String> {
    let root = root
        .canonicalize()
        .map_err(|err| format!("sandbox root is unavailable: {err}"))?;
    let relative = Path::new(relative);
    if relative.is_absolute()
        || relative.components().any(|part| {
            matches!(
                part,
                Component::ParentDir | Component::RootDir | Component::Prefix(_)
            )
        })
    {
        return Err("path escapes the configured sandbox".to_string());
    }
    let target = root
        .join(relative)
        .canonicalize()
        .map_err(|err| format!("sandbox path is unavailable: {err}"))?;
    if !target.starts_with(&root) {
        return Err("path escapes the configured sandbox".to_string());
    }
    Ok(target)
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
            guard.metrics = status.metrics.clone();
            guard.status = AgentStatus {
                running: status.running,
                replica_id: status.replica_id,
                root: Some(self.root.clone()),
                broker_connection: status.broker_connection.clone(),
            };
            match &status.connection {
                ConnectionState::Reconnecting {
                    attempt,
                    max_attempts,
                    ..
                } if !guard.degraded_notified => {
                    guard.degraded_notified = true;
                    Some(Toast::Degraded {
                        attempt: *attempt,
                        max_attempts: *max_attempts,
                    })
                }
                ConnectionState::ReenrollmentRequired { .. } if !guard.degraded_notified => {
                    guard.degraded_notified = true;
                    Some(Toast::Credential)
                }
                ConnectionState::Disconnected { attempts, .. } if !guard.disconnected_notified => {
                    guard.disconnected_notified = true;
                    Some(Toast::Disconnected {
                        attempts: *attempts,
                    })
                }
                ConnectionState::Connected if guard.degraded_notified => {
                    guard.degraded_notified = false;
                    Some(Toast::Recovered)
                }
                _ => None,
            }
        };
        match toast {
            Some(Toast::Degraded {
                attempt,
                max_attempts,
            }) => crate::notify::notify_degraded(&match max_attempts {
                Some(max) => format!(
                    "The broker is unreachable (attempt {attempt} of {max}); the agent stops if it \
                     runs out."
                ),
                None => "The broker is unreachable.".to_string(),
            }),
            Some(Toast::Recovered) => crate::notify::notify_recovered(),
            Some(Toast::Disconnected { attempts }) => crate::notify::notify_disconnected(attempts),
            Some(Toast::Credential) => crate::notify::notify_degraded(
                "The agent credential was rejected; re-enrollment is required.",
            ),
            None => {}
        }
    }

    fn on_worker_event(&self, event: &WorkerEvent) {
        log_line(&self.shared, describe_worker_event(event));
    }
}

// which health toast a connection transition warrants, if any.
enum Toast {
    Degraded {
        attempt: u32,
        max_attempts: Option<u32>,
    },
    Recovered,
    Disconnected {
        attempts: u32,
    },
    Credential,
}

/// render a worker-loop event as one console line, so the operator can see what this machine is
/// processing rather than only that the loop started.
fn describe_worker_event(event: &WorkerEvent) -> String {
    match event {
        WorkerEvent::EffectStarted {
            workflow_run_id,
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
                "Executing {provider}.{function} (run {}{attempt_suffix})...",
                short_id(workflow_run_id)
            )
        }
        WorkerEvent::EffectSkippedDuplicate { effect_id } => format!(
            "Skipped duplicate delivery for effect {}: it is already executing here.",
            short_id(effect_id)
        ),
        WorkerEvent::EffectFinished {
            workflow_run_id,
            provider,
            function,
            outcome,
            duration_ms,
            message,
            ..
        } => {
            let subject = format!("{provider}.{function} (run {})", short_id(workflow_run_id));
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
        effect_topic: DEFAULT_EFFECT_TOPIC.to_string(),
        infrastructure_effect_topic: DEFAULT_INFRASTRUCTURE_EFFECT_TOPIC.to_string(),
        control_topic: DEFAULT_CONTROL_TOPIC.to_string(),
        agent_topic: DEFAULT_AGENT_TOPIC.to_string(),
        effect_result_topic: DEFAULT_EFFECT_RESULT_TOPIC.to_string(),
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
        locator_mode: if config.discover {
            LocatorMode::Discover
        } else {
            LocatorMode::Static
        },
        gossip_bind: config.gossip_bind.clone(),
        gossip_port: config.gossip_port,
        api_key: config.api_key.clone(),
        enrollment_token: config.enrollment_token.clone(),
        credential_file: runinator_utilities::app_data::app_data_path(
            "agent/desktop-credential.json",
        )
        .unwrap_or_else(|_| std::path::PathBuf::from("desktop-credential.json")),
        outbox_file: runinator_utilities::app_data::app_data_path(
            "agent/desktop-result-outbox.jsonl",
        )
        .unwrap_or_else(|_| std::path::PathBuf::from("desktop-result-outbox.jsonl")),
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
        stale_after: Duration::from_secs(90),
        register_max_attempts: DEFAULT_REGISTER_MAX_ATTEMPTS,
        reconnect_max_attempts: config.reconnect_max_attempts,
        sample_telemetry: true,
        directive_handler: Arc::new(runinator_worker::agent::DefaultDirectiveHandler),
    })
}

/// kick off the agent on `rt`; returns immediately, updating `shared` as startup progresses. a no-op
/// if the agent is already running or mid-transition. a previous lifecycle that failed to come up
/// (or is parked waiting for re-enrollment) is stopped first, so Start can recover without a
/// process restart.
pub fn start(rt: &tokio::runtime::Handle, shared: SharedHandle, config: AgentConfig) {
    let generation;
    {
        let mut guard = shared.lock().expect("desktop agent state lock poisoned");
        if should_skip_start(&guard) {
            return;
        }
        guard.busy = true;
        guard.starting = true;
        guard.start_generation = guard.start_generation.wrapping_add(1);
        generation = guard.start_generation;
        guard.metrics = AgentMetrics::default();
        guard.connection = ConnectionState::Registering;
        guard.degraded_notified = false;
        guard.disconnected_notified = false;
        guard.status.running = false;
    }

    let task = {
        let shared = shared.clone();
        let rt = rt.clone();
        rt.clone().spawn(async move {
            start_inner(rt, shared, config, generation).await;
        })
    };
    // retain the startup task so a Cancel startup click can abort it wherever it is parked.
    let mut guard = shared.lock().expect("desktop agent state lock poisoned");
    guard.start_task = Some(task);
}

/// true when a Start click should be ignored: a transition is already in flight, or a live
/// lifecycle is actually running. a leftover handle from a failed start (or a parked re-enrollment
/// wait) must not count — that is what made Start a no-op after the first failure.
fn should_skip_start(shared: &Shared) -> bool {
    if shared.busy || shared.starting {
        return true;
    }
    let live = shared.handle.as_ref().is_some_and(|h| !h.is_finished());
    live && shared.status.running
}

async fn start_inner(
    rt: tokio::runtime::Handle,
    shared: SharedHandle,
    config: AgentConfig,
    generation: u64,
) {
    let leftover = {
        let mut guard = shared.lock().expect("desktop agent state lock poisoned");
        guard.handle.take()
    };
    if let Some(mut leftover) = leftover {
        leftover.shutdown();
        if let Err(err) = leftover.stop(STOP_GRACE).await {
            log_line(&shared, format!("Previous desktop agent exited: {err}"));
        }
    }

    configure_provider_environment(&config);
    let observer = Arc::new(DesktopObserver {
        shared: shared.clone(),
        root: config.sandbox_root.clone(),
    });

    let started = async {
        let mut runtime_config = runtime_config(&config)?;
        runtime_config.directive_handler = Arc::new(DesktopDirectiveHandler {
            shared: shared.clone(),
            root: PathBuf::from(&config.sandbox_root),
        });
        runinator_worker::prepare_agent_credentials(&mut runtime_config).await?;
        AgentRuntime::start(runtime_config, observer)
    }
    .await;

    let superseded = {
        let guard = shared.lock().expect("desktop agent state lock poisoned");
        guard.start_generation != generation
    };
    if superseded {
        // canceled (or restarted) while this attempt was coming up. tear down whatever it built
        // rather than publishing a lifecycle nothing is tracking.
        if let Ok(mut handle) = started {
            handle.shutdown();
            let _ = handle.stop(STOP_GRACE).await;
        }
        return;
    }

    let mut guard = shared.lock().expect("desktop agent state lock poisoned");
    guard.busy = false;
    guard.starting = false;
    match started {
        Ok(handle) => {
            if let Some(telemetry) = handle.telemetry() {
                spawn_telemetry_sampler(&rt, shared.clone(), telemetry, handle.watch());
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

    let Some(handle) = handle else {
        let mut guard = shared.lock().expect("desktop agent state lock poisoned");
        guard.busy = false;
        return;
    };
    // latch the stop before spawning, so an agent still in its registration backoff sees it.
    handle.shutdown();

    let shared_task = shared.clone();
    rt.spawn(async move {
        drain_and_settle(&shared_task, Some(handle), "Desktop agent stopped.").await;
    });
}

/// abandon a start that has not reached the action loop: abort the startup task and stop whatever it
/// managed to bring up. a no-op unless [`control_state`] reports [`Control::Starting`], so a stray
/// click cannot tear down a healthy agent.
///
/// this is the operator's way out of a startup that cannot finish — an unreachable service, a relay
/// that never accepts the credential, a registration budget still backing off — without killing the
/// process and losing the console.
pub fn cancel_start(rt: &tokio::runtime::Handle, shared: SharedHandle) {
    let task = {
        let mut guard = shared.lock().expect("desktop agent state lock poisoned");
        if control_state(&guard) != Control::Starting {
            return;
        }
        guard.starting = false;
        // hold the transition latch, so a Start or Stop click cannot interleave with the teardown.
        guard.busy = true;
        // invalidate the startup even if the abort below lands after it finished, or before the task
        // was recorded at all.
        guard.start_generation = guard.start_generation.wrapping_add(1);
        guard.start_task.take()
    };
    log_line(&shared, "Canceling desktop agent startup...");

    rt.spawn(async move {
        // the startup parks on network calls and registration backoff, so aborting is what makes the
        // cancel immediate rather than "once the current retry window elapses".
        if let Some(task) = task {
            task.abort();
            let _ = task.await;
        }
        // take the handle only once the startup task is finished: a lifecycle it stored on its way
        // out still has to be stopped rather than left running with nothing watching it.
        let handle = {
            let mut guard = shared.lock().expect("desktop agent state lock poisoned");
            guard.handle.take()
        };
        drain_and_settle(&shared, handle, "Desktop agent startup canceled.").await;
    });
}

/// Stop every lifecycle the desktop process owns before its Tokio runtime is dropped.
///
/// `AgentHandle` intentionally detaches its task when dropped, which is useful to hosts that keep
/// running.  It is the wrong default while this host itself is exiting: dropping the GUI runtime
/// while its asynchronously spawned drain is still pending can leave the worker lifecycle alive
/// after eframe has already closed.  Exit therefore aborts any incomplete startup, then drains the
/// published lifecycle synchronously on the GUI thread.
pub fn shutdown_for_process_exit(rt: &tokio::runtime::Runtime, shared: &SharedHandle) {
    let start_task = {
        let mut guard = shared.lock().expect("desktop agent state lock poisoned");
        // Prevent a startup task that wins a race with this shutdown from publishing a new handle.
        guard.start_generation = guard.start_generation.wrapping_add(1);
        guard.starting = false;
        guard.busy = true;
        guard.start_task.take()
    };

    if let Some(task) = start_task {
        task.abort();
        rt.block_on(async {
            let _ = task.await;
        });
    }

    // Take this only after the startup task has finished, so a just-completed startup cannot leave
    // its lifecycle in shared state after the process begins exiting.
    let handle = {
        let mut guard = shared.lock().expect("desktop agent state lock poisoned");
        guard.handle.take()
    };
    rt.block_on(drain_and_settle(
        shared,
        handle,
        "Desktop agent stopped for process exit.",
    ));
}

/// drain a live lifecycle and return the shared state to its stopped shape. shared by stop and
/// cancel, so a canceled startup settles exactly the way a stop does.
async fn drain_and_settle(shared: &SharedHandle, handle: Option<AgentHandle>, settled: &str) {
    if let Some(mut handle) = handle {
        handle.shutdown();
        if let Err(err) = handle.stop(STOP_GRACE).await {
            log_line(shared, format!("Desktop agent stop: {err}"));
        }
    }
    log_line(shared, settled);
    let mut guard = shared.lock().expect("desktop agent state lock poisoned");
    guard.status = AgentStatus::default();
    guard.connection = ConnectionState::Stopped;
    guard.metrics = AgentMetrics::default();
    guard.degraded_notified = false;
    guard.disconnected_notified = false;
    guard.busy = false;
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

#[cfg(test)]
#[path = "agent_tests.rs"]
mod tests;
