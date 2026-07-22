use std::sync::Arc;

use runinator_broker::Broker;
use runinator_database::interfaces::DatabaseImpl;
use runinator_models::errors::SendableError;
use tokio::sync::Notify;
use tokio::task::JoinSet;
use tracing::{error, info};

use crate::events::EnginePublisher;
use crate::loops::{
    run_action_dispatch_publisher, run_ingress_consumer, run_ready_node_reaper, run_replica_reaper,
    run_trigger_loop, run_usage_sampler, run_wake_publisher,
};
use crate::result_consumer::run_result_consumer;

/// run the durable orchestration engine: the ingress/reducer, result, wake, trigger, action-dispatch
/// loops plus the replica/ready-node/usage maintenance backstops. all loops share `shutdown`, and any
/// loop exiting on its own (panic or early return) fails the whole process so it restarts and resumes
/// from durable state rather than running on with a silently dead loop.
///
/// the engine is safe to run N-up: the broker consumers compete on shared consumer ids, the trigger
/// and action-dispatch loops claim disjoint rows per `instance_id`, wakes are broker-deduped, and the
/// reapers are idempotent.
pub async fn run_background_engine<T: DatabaseImpl>(
    pool: Arc<T>,
    broker: Arc<dyn Broker>,
    publisher: EnginePublisher,
    instance: String,
    shutdown: Arc<Notify>,
) -> Result<(), SendableError> {
    crate::stability::init_metrics();

    let mut loops: JoinSet<()> = JoinSet::new();
    loops.spawn(run_result_consumer(
        pool.clone(),
        broker.clone(),
        publisher.clone(),
        shutdown.clone(),
    ));
    loops.spawn(run_ingress_consumer(
        pool.clone(),
        broker.clone(),
        publisher.clone(),
        instance.clone(),
        shutdown.clone(),
    ));
    loops.spawn(run_wake_publisher(
        pool.clone(),
        broker.clone(),
        publisher.wake_nudge(),
        shutdown.clone(),
    ));
    loops.spawn(run_trigger_loop(
        pool.clone(),
        publisher.clone(),
        instance.clone(),
        shutdown.clone(),
    ));
    loops.spawn(run_action_dispatch_publisher(
        pool.clone(),
        broker.clone(),
        instance.clone(),
        publisher.action_nudge(),
        shutdown.clone(),
    ));
    loops.spawn(run_replica_reaper(pool.clone(), shutdown.clone()));
    loops.spawn(run_ready_node_reaper(pool.clone(), shutdown.clone()));
    loops.spawn(run_usage_sampler(pool.clone(), shutdown.clone()));

    info!("background engine started");
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
