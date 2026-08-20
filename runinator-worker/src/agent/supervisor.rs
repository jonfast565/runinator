//! restart-on-transient-failure around [`crate::start_worker_loop`].
//!
//! a fresh broker connection and [`crate::WorkerRuntime`] are rebuilt on every attempt: a broker
//! that failed to construct, or that died mid-run, does not get better by reusing the handle. a
//! graceful shutdown or a rejected immutable credential ends the retry loop; other exits are treated
//! as transient, so a machine nobody is watching keeps trying to rejoin instead of sitting there
//! "running" with a dead loop underneath.
//!
//! that patience is bounded by [`AgentRuntimeConfig::reconnect_max_attempts`]. an unreachable web
//! service or broker is not always temporary — the machine may have moved networks, or the cluster
//! may be gone — and an agent retrying forever keeps heartbeating a replica that can never take
//! work. once the budget is spent the agent reports [`AgentConnection::Disconnected`] and stops,
//! which retires the replica and (headless) exits the process non-zero for its supervisor to act on.
//! the counter is *consecutive*: an attempt that stays up past [`HEALTHY_AFTER`] clears it, so a
//! long-lived agent is never stopped by failures spread across days.

use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use runinator_api::{AsyncApiClient, StaticLocator};
use runinator_broker::ConnectionState as BrokerConnectionState;
use runinator_comm::ConsumerProfile;
use runinator_models::errors::SendableError;
use runinator_plugin::plugin::Plugin;
use uuid::Uuid;

use crate::agent::config::AgentRuntimeConfig;
use crate::agent::directives::DirectiveHandler;
use crate::agent::outbox::ResultOutbox;
use crate::agent::reconnect::ReconnectBudget;
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
// extra time allowed on top of the configured grace when unwinding a loop we decided to give up on,
// so a drain that is merely slow is not reported as a wedged one.
const GIVE_UP_DRAIN_SLACK: Duration = Duration::from_secs(5);

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
    /// consecutive failed attempts tolerated before giving up; `None` retries indefinitely.
    pub reconnect_max_attempts: Option<u32>,
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
            reconnect_max_attempts: (config.reconnect_max_attempts > 0)
                .then_some(config.reconnect_max_attempts),
        }
    }
}

/// drive the worker loop until shutdown, restarting it with backoff on any non-graceful exit.
///
/// `Ok(())` is a stop the agent was asked for (or parked on, in the re-enrollment case). `Err` means
/// the reconnect budget ran out: the agent stopped itself, which a headless host turns into a
/// non-zero exit for its supervisor to act on.
pub async fn run_supervised(
    inputs: SupervisedLoop,
    reporter: Arc<StatusReporter>,
    shutdown: Shutdown,
) -> Result<(), SendableError> {
    let mut retry_delay = RETRY_BASE;
    let budget = Arc::new(ReconnectBudget::new(inputs.reconnect_max_attempts));
    // one event sink shared by every restart, so counters survive a reconnect.
    let events: Arc<dyn WorkerEventSink> = {
        let reporter = Arc::clone(&reporter);
        Arc::new(move |event: WorkerEvent| reporter.worker_event(&event))
    };

    loop {
        if shutdown.is_stopping() {
            return Ok(());
        }
        reporter.set_connection(AgentConnection::Connecting);
        let broker = match crate::broker::build_broker(&inputs.broker_config).await {
            Ok(broker) => broker,
            Err(err) => {
                let charge = budget.charge(err.to_string());
                if charge.spent {
                    return give_up(&reporter, &shutdown, &budget);
                }
                reporter.log(format!(
                    "Failed to connect broker ({err}); retrying in {}s{}",
                    retry_delay.as_secs(),
                    attempt_suffix(&budget)
                ));
                if backoff(&reporter, &shutdown, &budget, &mut retry_delay).await {
                    return Ok(());
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
                Arc::clone(&budget),
                shutdown.clone(),
            ))
        });

        let runtime = WorkerRuntime {
            broker,
            profile: inputs.profile.clone(),
            libraries: Arc::clone(&inputs.libraries),
            api_client: inputs.api_client.clone(),
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

        // a transport that reconnects internally keeps this loop alive through an outage, so the
        // budget — not the loop's return — is what ends a hopeless episode; race the two.
        let mut worker_loop = std::pin::pin!(start_worker_loop(runtime));
        let exit = tokio::select! {
            result = &mut worker_loop => result,
            _ = budget.wait_spent() => {
                // latch the stop first so the loop unwinds and drains rather than being dropped
                // mid-action, then wait out its own grace period before giving up on it.
                shutdown.trigger();
                let drain = inputs.shutdown_grace + GIVE_UP_DRAIN_SLACK;
                if tokio::time::timeout(drain, &mut worker_loop).await.is_err() {
                    reporter.log("Worker loop did not drain within its grace period; stopping anyway.");
                }
                if let Some(task) = connection_monitor {
                    task.abort();
                }
                return give_up(&reporter, &shutdown, &budget);
            }
        };

        match exit {
            // a graceful return means shutdown was requested; nothing left to supervise.
            Ok(()) => return Ok(()),
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
                    return Ok(());
                }
                // an attempt that stayed up is evidence the endpoint works, so the outage this
                // budget is counting has ended even though this attempt failed.
                if started_at.elapsed() >= HEALTHY_AFTER {
                    retry_delay = RETRY_BASE;
                    budget.reset();
                }
                let charge = budget.charge(err.to_string());
                if let Some(task) = connection_monitor {
                    task.abort();
                }
                if charge.spent {
                    return give_up(&reporter, &shutdown, &budget);
                }
            }
        }

        reporter.log(format!(
            "Restarting worker loop in {}s{}...",
            retry_delay.as_secs(),
            attempt_suffix(&budget)
        ));
        if backoff(&reporter, &shutdown, &budget, &mut retry_delay).await {
            return Ok(());
        }
    }
}

/// stop the agent for good: publish the terminal state, latch shutdown so the heartbeat retires the
/// replica rather than advertising a worker that will never take another action, and hand back a
/// typed error.
fn give_up(
    reporter: &StatusReporter,
    shutdown: &Shutdown,
    budget: &ReconnectBudget,
) -> Result<(), SendableError> {
    let attempts = budget.attempts();
    let reason = budget.reason();
    reporter.update(|status| status.running = false);
    reporter.set_connection(AgentConnection::Disconnected {
        attempts,
        reason: reason.clone(),
    });
    reporter.log(format!(
        "Unreachable after {attempts} consecutive attempts ({reason}); disconnecting and stopping the agent."
    ));
    shutdown.trigger();
    Err(crate::errors::RECONNECT_EXHAUSTED
        .error(format!("{attempts} consecutive attempts: {reason}")))
}

// " (attempt 3 of 10)", or nothing at all when the budget is unlimited.
fn attempt_suffix(budget: &ReconnectBudget) -> String {
    match budget.max_attempts() {
        Some(max) => format!(" (attempt {} of {max})", budget.attempts()),
        None => String::new(),
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

/// mirror a self-reconnecting transport's own state into the agent's, charging the shared budget for
/// each failed dial. returns once the budget is spent, leaving [`run_supervised`] to publish the
/// terminal state — one place decides what giving up looks like.
async fn monitor_connection(
    mut state: tokio::sync::watch::Receiver<BrokerConnectionState>,
    reporter: Arc<StatusReporter>,
    budget: Arc<ReconnectBudget>,
    shutdown: Shutdown,
) {
    // when the transport last established a connection, so a *held* connection clears the budget but
    // a flapping one cannot. resetting the moment a dial succeeds would let an endpoint that accepts
    // the upgrade and immediately hangs up retry forever, one attempt at a time.
    let mut connected_since: Option<std::time::Instant> = None;
    loop {
        let mapped = match state.borrow().clone() {
            BrokerConnectionState::Idle | BrokerConnectionState::Connecting => {
                AgentConnection::Connecting
            }
            BrokerConnectionState::Connected => {
                connected_since = Some(std::time::Instant::now());
                AgentConnection::Connected
            }
            BrokerConnectionState::Reconnecting { retry_secs, reason } => {
                if connected_since
                    .take()
                    .is_some_and(|since| since.elapsed() >= HEALTHY_AFTER)
                {
                    budget.reset();
                }
                let charge = budget.charge(reason);
                if charge.spent {
                    return;
                }
                AgentConnection::Reconnecting {
                    retry_secs,
                    attempt: charge.attempt,
                    max_attempts: budget.max_attempts(),
                }
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
async fn backoff(
    reporter: &StatusReporter,
    shutdown: &Shutdown,
    budget: &ReconnectBudget,
    delay: &mut Duration,
) -> bool {
    reporter.set_connection(AgentConnection::Reconnecting {
        retry_secs: delay.as_secs(),
        attempt: budget.attempts(),
        max_attempts: budget.max_attempts(),
    });
    if shutdown.sleep_or_stop(*delay).await {
        return true;
    }
    *delay = (*delay * 2).min(RETRY_MAX);
    false
}
