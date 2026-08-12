//! restart-on-transient-failure around [`crate::start_worker_loop`].
//!
//! a fresh broker connection and [`crate::WorkerRuntime`] are rebuilt on every attempt: a broker
//! that failed to construct, or that died mid-run, does not get better by reusing the handle. only a
//! graceful shutdown or a rejected immutable credential ends the retry loop; other exits are treated
//! as transient, so a machine nobody is watching keeps trying to rejoin instead of sitting there
//! "running" with a dead loop underneath.

use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use runinator_api::{AsyncApiClient, StaticLocator};
use runinator_broker::ConnectionState as BrokerConnectionState;
use runinator_comm::ConsumerProfile;
use runinator_plugin::plugin::Plugin;
use uuid::Uuid;

use crate::agent::config::AgentRuntimeConfig;
use crate::agent::directives::DirectiveHandler;
use crate::agent::outbox::ResultOutbox;
use crate::agent::reporter::StatusReporter;
use crate::agent::shutdown::Shutdown;
use crate::agent::status::AgentConnection;
use crate::broker::BrokerConfig;
use crate::events::{WorkerEvent, WorkerEventSink};
use crate::provider_repository::ProviderFactory;
use crate::worker::{WorkerRuntime, start_worker_loop};

// backoff for restarting the loop after a failed attempt; grows, capped, and resets once a run has
// stayed up long enough to call it healthy.
const RETRY_BASE: Duration = Duration::from_secs(2);
const RETRY_MAX: Duration = Duration::from_secs(60);
const HEALTHY_AFTER: Duration = Duration::from_secs(60);

/// the per-attempt inputs, assembled once by the lifecycle and cloned into each restart.
pub struct SupervisedLoop {
    pub broker_config: BrokerConfig,
    pub profile: ConsumerProfile,
    pub api_client: AsyncApiClient<StaticLocator>,
    pub replica_id: Uuid,
    pub providers: ProviderFactory,
    pub libraries: Arc<HashMap<String, Plugin>>,
    pub max_concurrent_actions: usize,
    pub shutdown_grace: Duration,
    pub result_outbox: Arc<dyn ResultOutbox>,
    pub directive_handler: Arc<dyn DirectiveHandler>,
}

impl SupervisedLoop {
    /// build the per-attempt inputs from the agent config and a registered replica identity.
    pub fn new(
        config: &AgentRuntimeConfig,
        api_client: AsyncApiClient<StaticLocator>,
        replica_id: Uuid,
        libraries: Arc<HashMap<String, Plugin>>,
        result_outbox: Arc<dyn ResultOutbox>,
    ) -> Self {
        // carry the replica id (without exclusivity unless asked) so replica-targeted actions —
        // and cancels routed to the worker holding an action's executor lease — reach this agent.
        let consumer_id = config
            .consumer_id
            .clone()
            .unwrap_or_else(|| replica_id.to_string());
        let mut profile = ConsumerProfile::shared(consumer_id)
            .with_replica_id(replica_id)
            .with_labels(config.labels.clone());
        if config.exclusive {
            profile = profile.exclusive();
        }

        Self {
            broker_config: config.broker.clone(),
            profile,
            api_client,
            replica_id,
            providers: Arc::clone(&config.providers),
            libraries,
            max_concurrent_actions: config.max_concurrent_actions.max(1),
            shutdown_grace: config.shutdown_grace,
            result_outbox,
            directive_handler: Arc::clone(&config.directive_handler),
        }
    }
}

/// drive the worker loop until shutdown, restarting it with backoff on any non-graceful exit.
pub async fn run_supervised(
    inputs: SupervisedLoop,
    reporter: Arc<StatusReporter>,
    shutdown: Shutdown,
) {
    let mut retry_delay = RETRY_BASE;
    // one event sink shared by every restart, so counters survive a reconnect.
    let events: Arc<dyn WorkerEventSink> = {
        let reporter = Arc::clone(&reporter);
        Arc::new(move |event: WorkerEvent| reporter.worker_event(&event))
    };

    loop {
        if shutdown.is_stopping() {
            return;
        }
        reporter.set_connection(AgentConnection::Connecting);
        let broker = match crate::broker::build_broker(&inputs.broker_config).await {
            Ok(broker) => broker,
            Err(err) => {
                reporter.log(format!(
                    "Failed to connect broker ({err}); retrying in {}s",
                    retry_delay.as_secs()
                ));
                if backoff(&reporter, &shutdown, &mut retry_delay).await {
                    return;
                }
                continue;
            }
        };
        let broker_state = broker.connection_state();
        let has_connection_state = broker_state.is_some();
        let connection_monitor = broker_state.clone().map(|state| {
            tokio::spawn(monitor_connection(
                state,
                Arc::clone(&reporter),
                shutdown.clone(),
            ))
        });

        let runtime = WorkerRuntime {
            broker,
            profile: inputs.profile.clone(),
            libraries: Arc::clone(&inputs.libraries),
            api_client: inputs.api_client.clone(),
            replica_id: Some(inputs.replica_id),
            providers: Arc::clone(&inputs.providers),
            max_concurrent_actions: inputs.max_concurrent_actions,
            shutdown_grace: inputs.shutdown_grace,
            shutdown: shutdown.notify(),
            events: Arc::clone(&events),
            result_outbox: Arc::clone(&inputs.result_outbox),
            directive_handler: Arc::clone(&inputs.directive_handler),
        };

        if !has_connection_state {
            reporter.set_connection(AgentConnection::Connected);
        }
        reporter.update(|status| status.running = true);
        let started_at = std::time::Instant::now();
        match start_worker_loop(runtime).await {
            // a graceful return means shutdown was requested; nothing left to supervise.
            Ok(()) => return,
            Err(err) => {
                reporter.log(format!("Worker loop ended with an error: {err}"));
                if let Some(reason) = unauthorized_reason(broker_state.as_ref()) {
                    if let Some(task) = connection_monitor {
                        task.abort();
                    }
                    reporter.update(|status| status.running = false);
                    reporter.set_connection(AgentConnection::ReenrollmentRequired { reason });
                    reporter
                        .log("Broker rejected the agent credential; waiting for re-enrollment.");
                    while !shutdown.sleep_or_stop(Duration::from_secs(60 * 60)).await {}
                    return;
                }
                if started_at.elapsed() >= HEALTHY_AFTER {
                    retry_delay = RETRY_BASE;
                }
            }
        }
        if let Some(task) = connection_monitor {
            task.abort();
        }

        reporter.log(format!(
            "Restarting worker loop in {}s...",
            retry_delay.as_secs()
        ));
        if backoff(&reporter, &shutdown, &mut retry_delay).await {
            return;
        }
    }
}

fn unauthorized_reason(
    state: Option<&tokio::sync::watch::Receiver<BrokerConnectionState>>,
) -> Option<String> {
    match state?.borrow().clone() {
        BrokerConnectionState::Unauthorized { reason } => Some(reason),
        _ => None,
    }
}

async fn monitor_connection(
    mut state: tokio::sync::watch::Receiver<BrokerConnectionState>,
    reporter: Arc<StatusReporter>,
    shutdown: Shutdown,
) {
    loop {
        let mapped = match state.borrow().clone() {
            BrokerConnectionState::Idle | BrokerConnectionState::Connecting => {
                AgentConnection::Connecting
            }
            BrokerConnectionState::Connected => AgentConnection::Connected,
            BrokerConnectionState::Reconnecting { retry_secs, .. } => {
                AgentConnection::Reconnecting { retry_secs }
            }
            BrokerConnectionState::Unauthorized { reason } => {
                AgentConnection::ReenrollmentRequired { reason }
            }
        };
        reporter.set_connection(mapped);

        let shutdown_notify = shutdown.notify();
        tokio::select! {
            _ = shutdown_notify.notified() => return,
            changed = state.changed() => {
                if changed.is_err() {
                    return;
                }
            }
        }
    }
}

// publish the reconnecting phase, wait out the delay, and grow it. returns true when shutdown fired.
async fn backoff(reporter: &StatusReporter, shutdown: &Shutdown, delay: &mut Duration) -> bool {
    reporter.set_connection(AgentConnection::Reconnecting {
        retry_secs: delay.as_secs(),
    });
    if shutdown.sleep_or_stop(*delay).await {
        return true;
    }
    *delay = (*delay * 2).min(RETRY_MAX);
    false
}
