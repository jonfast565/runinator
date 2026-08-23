//! the shared agent lifecycle: register, publish providers, heartbeat, then supervise the worker
//! loop until shutdown. the standalone binary and the desktop agent both run exactly this, so the
//! only difference between a headless agent and a tray one is which [`AgentObserver`] is attached.

use std::sync::Arc;
use std::time::Duration;

use runinator_api::{AsyncApiClient, StaticLocator};
use runinator_models::errors::SendableError;
use runinator_observability::resource_telemetry::TelemetryCollector;
use runinator_platform::liveness;
use tokio::sync::watch;
use tokio::task::JoinHandle;
use uuid::Uuid;

use crate::agent::config::AgentRuntimeConfig;
use crate::agent::observer::AgentObserver;
use crate::agent::outbox::{FileOutbox, ResultOutbox};
use crate::agent::registration::{announce_agent_replica, spawn_agent_heartbeat};
use crate::agent::reporter::StatusReporter;
use crate::agent::shutdown::Shutdown;
use crate::agent::status::{AgentConnection, AgentReportContext, AgentStatus};
use crate::agent::supervisor::{SupervisedLoop, run_supervised};
use crate::worker::load_libraries;

/// entry point for hosting an agent.
pub struct AgentRuntime;

impl AgentRuntime {
    /// start the lifecycle and return immediately. must be called from within a tokio runtime
    /// context.
    ///
    /// only configuration that cannot be retried fails here (an unusable service URL, an unreadable
    /// plugin path). anything the agent could recover from — the service being down, the broker
    /// being unreachable — is retried inside the lifecycle and reported through `observer`, so a
    /// host never has to implement its own retry policy to be robust.
    pub fn start(
        config: AgentRuntimeConfig,
        observer: Arc<dyn AgentObserver>,
    ) -> Result<AgentHandle, SendableError> {
        let api_client = AsyncApiClient::with_credentials(
            StaticLocator::new(config.service_url.clone()),
            config.api_key.clone(),
        )
        .map_err(|err| crate::errors::API_CLIENT.error(err))?;
        let libraries = Arc::new(load_libraries(&config.dll_paths)?);
        let result_outbox: Arc<dyn ResultOutbox> = Arc::new(
            FileOutbox::open(&config.outbox_file)
                .map_err(|err| crate::errors::API_CLIENT.error(err))?,
        );

        let report_context = Arc::new(AgentReportContext::new(
            &config,
            (config.providers)().len(),
            Arc::clone(&result_outbox),
        ));
        let reporter = Arc::new(StatusReporter::new(
            observer,
            AgentStatus {
                running: false,
                replica_id: None,
                connection: AgentConnection::Registering,
                broker_connection: Some(config.broker_description.clone()),
                metrics: Default::default(),
                last_error: None,
                last_error_at: None,
            },
        ));
        let telemetry = config
            .sample_telemetry
            .then(|| Arc::new(TelemetryCollector::new()));
        let shutdown = Shutdown::new();
        let state = reporter.subscribe();

        let task = tokio::spawn(run_lifecycle(
            config,
            api_client,
            libraries,
            telemetry.clone(),
            report_context,
            result_outbox,
            Arc::clone(&reporter),
            shutdown.clone(),
        ));

        Ok(AgentHandle {
            shutdown,
            task,
            state,
            telemetry,
        })
    }
}

/// a running agent. dropping it detaches the lifecycle rather than stopping it; call
/// [`AgentHandle::shutdown`] or [`AgentHandle::stop`] to actually stop.
pub struct AgentHandle {
    shutdown: Shutdown,
    task: JoinHandle<Result<(), SendableError>>,
    state: watch::Receiver<AgentStatus>,
    telemetry: Option<Arc<TelemetryCollector>>,
}

impl AgentHandle {
    /// request shutdown without waiting. safe to call more than once, and before the lifecycle has
    /// reached any particular stage.
    pub fn shutdown(&self) {
        self.shutdown.trigger();
    }

    /// await the lifecycle's own exit. returns what it returned: an error only when the agent could
    /// not be brought up at all.
    pub async fn wait(&mut self) -> Result<(), SendableError> {
        match (&mut self.task).await {
            Ok(result) => result,
            Err(err) if err.is_cancelled() => Ok(()),
            Err(err) => Err(crate::errors::LOOP_JOIN.error(err)),
        }
    }

    /// request shutdown and drain within `grace`, abandoning the task if it overruns. the worker
    /// loop bounds its own in-flight work, so an overrun means something below it is wedged.
    pub async fn stop(&mut self, grace: Duration) -> Result<(), SendableError> {
        self.shutdown.trigger();
        match tokio::time::timeout(grace, self.wait()).await {
            Ok(result) => result,
            Err(_) => {
                self.task.abort();
                Err(crate::errors::SHUTDOWN_TIMEOUT.error(format!("{}s", grace.as_secs())))
            }
        }
    }

    pub fn status(&self) -> AgentStatus {
        self.state.borrow().clone()
    }

    pub fn replica_id(&self) -> Option<Uuid> {
        self.state.borrow().replica_id
    }

    /// watch lifecycle transitions without implementing an observer.
    pub fn watch(&self) -> watch::Receiver<AgentStatus> {
        self.state.clone()
    }

    /// the host telemetry collector, when this agent samples it. exposed so a host can mirror
    /// cpu/memory at its own cadence rather than the heartbeat's.
    pub fn telemetry(&self) -> Option<Arc<TelemetryCollector>> {
        self.telemetry.clone()
    }

    pub fn is_finished(&self) -> bool {
        self.task.is_finished()
    }
}

async fn run_lifecycle(
    config: AgentRuntimeConfig,
    api_client: AsyncApiClient<StaticLocator>,
    libraries: Arc<std::collections::HashMap<String, runinator_plugin::plugin::Plugin>>,
    telemetry: Option<Arc<TelemetryCollector>>,
    report_context: Arc<AgentReportContext>,
    result_outbox: Arc<dyn ResultOutbox>,
    reporter: Arc<StatusReporter>,
    shutdown: Shutdown,
) -> Result<(), SendableError> {
    reporter.log(format!("Connecting to {} ...", config.service_url));
    reporter.set_connection(AgentConnection::Registering);

    let liveness_task = liveness::spawn_liveness(
        &config.liveness_file,
        liveness::DEFAULT_LIVENESS_INTERVAL,
        shutdown.notify(),
    );

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
    let availability_heartbeat = spawn_agent_heartbeat(
        presence_broker,
        &config,
        replica_id,
        runtime_id,
        Arc::clone(&reporter),
        report_context,
        telemetry,
        shutdown.clone(),
    );
    reporter.log(format!("Broker: {}", config.broker_description));

    let inputs = SupervisedLoop::new(&config, api_client, replica_id, libraries, result_outbox);
    let outcome = run_supervised(inputs, Arc::clone(&reporter), shutdown.clone()).await;

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
