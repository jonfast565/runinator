//! the shared agent lifecycle: register, publish providers, heartbeat, then supervise the worker
//! loop until shutdown. the standalone binary and the desktop agent both run exactly this, so the
//! only difference between a headless agent and a tray one is which [`AgentObserver`] is attached.

use std::sync::Arc;
use std::time::Duration;

use runinator_api::{AsyncApiClient, StaticLocator};
use runinator_models::{errors::SendableError, server_settings::WorkerSettings};
use runinator_observability::resource_telemetry::TelemetryCollector;
use runinator_platform::liveness;
use tokio::sync::watch;
use tokio::task::JoinHandle;
use uuid::Uuid;

use crate::agent::config::AgentRuntimeConfig;
use crate::agent::observer::AgentObserver;
use crate::agent::outbox::{FileOutbox, ResultOutbox};
use crate::agent::registration::{
    AgentAvailability, AgentHeartbeat, announce_agent_replica, spawn_agent_heartbeat,
};
use crate::agent::reporter::StatusReporter;
use crate::agent::shutdown::Shutdown;
use crate::agent::status::{AgentConnection, AgentReportContext, AgentStatus};
use crate::agent::supervisor::{SupervisedLoop, run_supervised};
use crate::worker::load_libraries;

/// entry point for hosting an agent.

/// a running agent. dropping it detaches the lifecycle rather than stopping it; call
/// [`AgentHandle::shutdown`] or [`AgentHandle::stop`] to actually stop.

async fn run_lifecycle(lifecycle: AgentLifecycle) -> Result<(), SendableError> {
    let AgentLifecycle {
        mut config,
        api_client,
        libraries,
        telemetry,
        report_context,
        result_outbox,
        reporter,
        shutdown,
    } = lifecycle;
    reporter.log(format!("Connecting to {} ...", config.service_url));
    reporter.set_connection(AgentConnection::Registering);

    let liveness_task = liveness::spawn_liveness(
        &config.liveness_file,
        liveness::DEFAULT_LIVENESS_INTERVAL,
        shutdown.notify(),
    );

    let mut using_server_worker_settings = false;
    if config.use_server_worker_settings {
        match api_client.worker_settings().await {
            Ok(policy) if policy.configured => {
                apply_worker_settings(&mut config, &policy.values);
                using_server_worker_settings = true;
                report_context.update_worker_settings(&config, "server");
                reporter.log("Applied platform worker settings.");
            }
            Ok(_) => reporter.log("No platform worker settings are saved; using process settings."),
            Err(error) => reporter.log(format!(
                "Could not load platform worker settings; using process settings: {error}"
            )),
        }
    }

    // A broker-announced replica knows its identity before the asynchronous ingress consumer
    // writes the row. That same id is safe to put in effect claims and targeted broker profiles.
    let replica_id = Uuid::now_v7();
    let runtime_id = replica_id.to_string();
    let presence_broker = match crate::broker::build_broker(&config.broker).await {
        Ok(broker) => broker,
        Err(err) => {
            settle(&reporter, liveness_task);
            return Err(err);
        }
    };
    if let Err(err) = announce_agent_replica(
        presence_broker.as_ref(),
        &config,
        reporter.as_ref(),
        report_context.as_ref(),
        replica_id,
        &runtime_id,
    )
    .await
    {
        settle(&reporter, liveness_task);
        return Err(Box::new(err));
    }
    reporter.update(|status| status.replica_id = Some(replica_id));
    reporter.log(format!("Announced broker replica {replica_id}."));
    if !config.labels.is_empty() {
        // surfacing the advertised labels makes "which agent did this go to" answerable from the
        // agent's own output: a label-targeted action only routes here when these satisfy it.
        let rendered = config
            .labels
            .iter()
            .map(|(key, value)| format!("{key}={value}"))
            .collect::<Vec<_>>()
            .join(", ");
        reporter.log(format!("Advertising labels: {rendered}"));
    }

    // The broker heartbeat keeps the replica live and marks it offline on shutdown.
    let availability_heartbeat = spawn_agent_heartbeat(AgentHeartbeat {
        broker: presence_broker,
        availability: AgentAvailability::from_config(&config),
        heartbeat_interval: config.heartbeat_interval,
        replica_id,
        runtime_id,
        reporter: Arc::clone(&reporter),
        report_context: Arc::clone(&report_context),
        telemetry,
        shutdown: shutdown.clone(),
    });
    reporter.log(format!("Broker: {}", config.broker_description));

    let outcome = loop {
        if shutdown.is_stopping() {
            break Ok(());
        }

        let inputs = SupervisedLoop::new(
            &config,
            api_client.clone(),
            replica_id,
            Arc::clone(&libraries),
            Arc::clone(&result_outbox),
        );
        if !config.use_server_worker_settings {
            break run_supervised(inputs, Arc::clone(&reporter), shutdown.clone()).await;
        }

        let attempt_shutdown = Shutdown::new();
        let mut supervised = std::pin::pin!(run_supervised(
            inputs,
            Arc::clone(&reporter),
            attempt_shutdown.clone(),
        ));
        let current = worker_settings_from_config(&config);
        let refresh = wait_for_worker_settings_change(
            &api_client,
            &current,
            using_server_worker_settings,
            &reporter,
            config.workers_refresh_interval(),
        );
        tokio::pin!(refresh);
        let shutdown_notify = shutdown.notify();

        tokio::select! {
            result = &mut supervised => break result,
            _ = shutdown_notify.notified() => {
                attempt_shutdown.trigger();
                break supervised.await;
            }
            next = &mut refresh => {
                reporter.log("Worker settings changed; draining before applying them.");
                attempt_shutdown.trigger();
                if let Err(error) = supervised.await {
                    reporter.log(format!("Worker loop stopped while applying settings: {error}"));
                }
                apply_worker_settings(&mut config, &next);
                using_server_worker_settings = true;
                report_context.update_worker_settings(&config, "server");
                reporter.log(format!(
                    "Applied worker settings: {} concurrent actions, {}s shutdown grace.",
                    config.max_concurrent_actions,
                    config.shutdown_grace.as_secs(),
                ));
            }
        }
    };

    // An intentional stop is normally already latched, but an unexpected terminal loop result
    // must also retire the broker-announced replica. Wait briefly so the offline message has a
    // chance to reach the transport before a standalone process exits.
    shutdown.trigger();
    match tokio::time::timeout(Duration::from_secs(5), availability_heartbeat).await {
        Ok(Ok(())) => {}
        Ok(Err(err)) => reporter.record_error(format!("availability heartbeat stopped: {err}")),
        Err(_) => reporter.record_error("timed out retiring broker replica"),
    }

    settle(&reporter, liveness_task);
    // an exhausted reconnect budget is the agent stopping itself, not a clean stop; propagate it so
    // a headless host exits non-zero and a gui host can say why the agent is no longer running.
    outcome?;
    reporter.log("Agent stopped.");
    Ok(())
}

impl AgentRuntimeConfig {
    fn workers_refresh_interval(&self) -> Duration {
        Duration::from_secs(worker_settings_from_config(self).settings_refresh_interval_seconds)
    }
}

fn worker_settings_from_config(config: &AgentRuntimeConfig) -> WorkerSettings {
    WorkerSettings {
        max_concurrent_actions: config.max_concurrent_actions as u64,
        shutdown_grace_seconds: config.shutdown_grace.as_secs(),
        reconnect_max_attempts: config.reconnect_max_attempts as u64,
        settings_refresh_interval_seconds: config.worker_settings_refresh_interval.as_secs(),
    }
}

fn apply_worker_settings(config: &mut AgentRuntimeConfig, settings: &WorkerSettings) {
    config.max_concurrent_actions = usize::try_from(settings.max_concurrent_actions)
        .unwrap_or(usize::MAX)
        .max(1);
    config.shutdown_grace = Duration::from_secs(settings.shutdown_grace_seconds.max(1));
    config.reconnect_max_attempts =
        u32::try_from(settings.reconnect_max_attempts).unwrap_or(u32::MAX);
    config.worker_settings_refresh_interval =
        Duration::from_secs(settings.settings_refresh_interval_seconds.max(1));
}

async fn wait_for_worker_settings_change(
    api_client: &AsyncApiClient<StaticLocator>,
    current: &WorkerSettings,
    using_server_settings: bool,
    reporter: &StatusReporter,
    mut interval: Duration,
) -> WorkerSettings {
    loop {
        tokio::time::sleep(interval).await;
        match api_client.worker_settings().await {
            Ok(policy) if policy.configured => {
                interval = Duration::from_secs(policy.values.settings_refresh_interval_seconds);
                if !using_server_settings || policy.values != *current {
                    return policy.values;
                }
            }
            Ok(_) => {}
            Err(error) => reporter.log(format!("Could not refresh worker settings: {error}")),
        }
    }
}

// return the status to its terminal shape and stop touching the liveness file, so a stopped agent
// never looks alive to an exec probe.
fn settle(reporter: &StatusReporter, liveness_task: Option<JoinHandle<()>>) {
    if let Some(task) = liveness_task {
        task.abort();
    }
    reporter.update(|status| {
        status.running = false;
        // `Disconnected` is a terminal state of its own; overwriting it with `Stopped` would make an
        // agent that gave up indistinguishable from one an operator stopped.
        if !matches!(status.connection, AgentConnection::Disconnected { .. }) {
            status.connection = AgentConnection::Stopped;
        }
    });
}

mod agent_runtime;
pub use agent_runtime::AgentRuntime;

mod agent_handle;
pub use agent_handle::AgentHandle;

mod agent_lifecycle;
use agent_lifecycle::AgentLifecycle;
