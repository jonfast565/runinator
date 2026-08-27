use std::sync::Arc;

use runinator_broker_core::{Broker, EmbeddedEngineSignals};
use runinator_models::errors::SendableError;
use runinator_store::{
    RuntimeStore,
    roles::{
        DefinitionStore, IngressStore, NotificationStore, OrchestrationStore, OrgStore,
        ReplicaStore, RunStore, ScheduleStore, WorkflowVmStore, WorkspaceStore,
    },
};
use tokio::sync::Notify;
use tokio::task::JoinSet;
use tracing::{error, info};

use crate::events::EventSender;
use crate::loops::{
    run_agent_directive_publisher, run_notification_effect_dispatcher,
    run_operational_metrics_sampler, run_replica_reaper, run_timer_interrupt_scheduler,
    run_trigger_loop, run_usage_sampler, run_workflow_effect_dispatcher, run_workflow_vm_driver,
    run_workspace_reconciler,
};
use crate::settings::{ServerSettingsHandle, run_server_settings_refresher};

/// Runtime limits for one durable engine instance.
///
/// The ingress limit bounds continuation drives, control commands, and agent directive results that may
/// be processed concurrently. Durable ready-node claims and run-state compare-and-swap writes
/// remain the authority for conflicting work.
#[derive(Debug, Clone, Copy)]
pub struct EngineConfig {
    pub max_concurrent_ingress: usize,
}

impl Default for EngineConfig {
    fn default() -> Self {
        Self {
            max_concurrent_ingress: 16,
        }
    }
}

impl EngineConfig {
    pub fn normalized(self) -> Self {
        Self {
            max_concurrent_ingress: self.max_concurrent_ingress.max(1),
        }
    }
}

/// Persistence required by the background-engine lifecycle.
///
/// This is a use-case boundary, not a catch-all repository: it names the durable queues and
/// records the long-running orchestration loops coordinate. Authentication, function packages,
/// workflow history, artifacts, and schema initialization deliberately stay outside it.
pub trait BackgroundEngineStore:
    RuntimeStore
    + WorkflowVmStore
    + RunStore
    + NotificationStore
    + ReplicaStore
    + OrgStore
    + ScheduleStore
    + DefinitionStore
    + IngressStore
    + WorkspaceStore
    + OrchestrationStore
{
}

impl<T> BackgroundEngineStore for T where
    T: RuntimeStore
        + WorkflowVmStore
        + RunStore
        + NotificationStore
        + ReplicaStore
        + OrgStore
        + ScheduleStore
        + DefinitionStore
        + IngressStore
        + WorkspaceStore
        + OrchestrationStore
{
}

#[cfg(test)]
mod tests {
    use super::EngineConfig;

    #[test]
    fn ingress_concurrency_defaults_to_a_bounded_parallel_limit() {
        assert_eq!(EngineConfig::default().max_concurrent_ingress, 16);
        assert_eq!(
            EngineConfig {
                max_concurrent_ingress: 0,
            }
            .normalized()
            .max_concurrent_ingress,
            1
        );
    }
}

/// Run the durable VM orchestration engine.
///
/// Continuation and effect outboxes are the only workflow execution queues. In particular, this
/// deliberately does not start the pre-VM action-dispatch loop: starting either execution engine
/// alongside the VM would make the cutover's exactly-once ownership guarantees meaningless.
///
/// The ingress consumer *is* started: ingress carries traffic toward the engine (a due timer wake
/// from the waker, a worker control request, an agent directive reply), none of which belongs to
/// the retired execution engine.
///
/// the engine is safe to run N-up: the broker consumers compete on shared consumer ids, the trigger
/// and notification-effect loops claim disjoint rows per `instance_id`, and the reapers are
/// idempotent.
pub async fn run_background_engine<T: BackgroundEngineStore>(
    pool: Arc<T>,
    broker: Arc<dyn Broker>,
    publisher: EventSender,
    local_signals: Option<EmbeddedEngineSignals>,
    instance: String,
    config: EngineConfig,
    shutdown: Arc<Notify>,
) -> Result<(), SendableError> {
    crate::stability::init_metrics();
    let config = config.normalized();
    runinator_observability::tui::register(
        "engine",
        [format!(
            "durable orchestration  •  ingress concurrency {}",
            config.max_concurrent_ingress
        )],
    );
    runinator_observability::tui::gauge(
        "engine",
        "ingress capacity",
        config.max_concurrent_ingress as i64,
    );
    runinator_observability::tui::activity("engine", "starting durable orchestration loops", None);
    // A standalone engine owns no HTTP-side signals; its durable loops continue polling normally.
    let local_signals = local_signals.unwrap_or_default();
    let server_settings = ServerSettingsHandle::load(pool.as_ref()).await?;
    let orchestration_nudge = Arc::new(Notify::new());

    let mut loops: JoinSet<()> = JoinSet::new();
    loops.spawn(run_server_settings_refresher(
        pool.clone(),
        server_settings.clone(),
        shutdown.clone(),
    ));
    loops.spawn(crate::effect_consumer::run_effect_result_consumer(
        pool.clone(),
        broker.clone(),
        publisher.clone(),
        shutdown.clone(),
    ));
    loops.spawn(crate::run_infrastructure_effect_host(
        pool.clone(),
        broker.clone(),
        shutdown.clone(),
    ));
    loops.spawn(
        crate::ingress_consumer::run_ingress_consumer_with_orchestration_nudge(
            pool.clone(),
            broker.clone(),
            orchestration_nudge.clone(),
            shutdown.clone(),
        ),
    );
    loops.spawn(run_trigger_loop(
        pool.clone(),
        publisher.clone(),
        instance.clone(),
        server_settings.clone(),
        shutdown.clone(),
    ));
    loops.spawn(run_agent_directive_publisher(
        pool.clone(),
        broker.clone(),
        instance.clone(),
        local_signals.agent_directives_notifier(),
        server_settings.clone(),
        shutdown.clone(),
    ));
    loops.spawn(run_workflow_vm_driver(
        pool.clone(),
        instance.clone(),
        local_signals.workflow_vm_notifier(),
        server_settings.clone(),
        shutdown.clone(),
    ));
    loops.spawn(crate::loops::run_correlated_orchestration_reducer(
        pool.clone(),
        broker.clone(),
        publisher.clone(),
        instance.clone(),
        orchestration_nudge,
        server_settings.clone(),
        shutdown.clone(),
    ));
    loops.spawn(run_timer_interrupt_scheduler(
        pool.clone(),
        broker.clone(),
        server_settings.clone(),
        shutdown.clone(),
    ));
    loops.spawn(run_workflow_effect_dispatcher(
        pool.clone(),
        broker.clone(),
        publisher.clone(),
        instance.clone(),
        server_settings.clone(),
        shutdown.clone(),
    ));
    loops.spawn(run_notification_effect_dispatcher(
        pool.clone(),
        broker.clone(),
        instance.clone(),
        server_settings.clone(),
        shutdown.clone(),
    ));
    loops.spawn(run_replica_reaper(
        pool.clone(),
        server_settings.clone(),
        shutdown.clone(),
    ));
    loops.spawn(run_workspace_reconciler(
        pool.clone(),
        publisher.clone(),
        server_settings.clone(),
        shutdown.clone(),
    ));
    loops.spawn(run_usage_sampler(
        pool.clone(),
        server_settings.clone(),
        shutdown.clone(),
    ));
    loops.spawn(run_operational_metrics_sampler(
        pool.clone(),
        server_settings.clone(),
        shutdown.clone(),
    ));
    loops.spawn(crate::notifications::run_notification_scanner(
        pool.clone(),
        publisher.clone(),
        server_settings,
        shutdown.clone(),
    ));

    info!("background engine started");
    runinator_observability::tui::activity("engine", "waiting for durable work", None);
    tokio::select! {
        // graceful shutdown is checked first so normal teardown is never misreported as a failure.
        biased;
        _ = shutdown.notified() => {
            info!("shutting down background engine...");
            loops.shutdown().await;
            Ok(())
        }
        Some(joined) = loops.join_next() => {
            match &joined {
                Err(err) if err.is_panic() => {
                    error!("background orchestration loop panicked; shutting down: {err}");
                }
                Err(err) => {
                    error!("background orchestration loop aborted; shutting down: {err}");
                }
                Ok(()) => {
                    error!("background orchestration loop exited unexpectedly; shutting down");
                }
            }
            crate::stability::record_background_loop_failure();
            shutdown.notify_waiters();
            loops.shutdown().await;
            Err(crate::errors::BACKGROUND_LOOP_EXITED.bare())
        }
    }
}
