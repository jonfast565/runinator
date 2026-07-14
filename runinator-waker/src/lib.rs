pub mod config;
pub mod errors;
pub mod metrics;

use std::sync::Arc;
use std::time::Duration;

use chrono::Utc;
use runinator_broker::{Broker, IngressMessage, WsIngressCommand};
use runinator_models::errors::error_code_or_unknown;
use tokio::sync::{Notify, Semaphore};
use tokio::task::JoinSet;
use tracing::{Instrument, error, info};

use crate::config::Config;

// backoff before retrying a failed wake receive, so a broker outage does not hot-loop the waker.
const RECEIVE_RETRY_BACKOFF: Duration = Duration::from_millis(250);

/// touches the configured liveness file on an interval until shutdown; used by the k8s exec probe.
/// returns none when no liveness file is configured.
pub fn spawn_liveness(
    config: &Config,
    shutdown: Arc<Notify>,
) -> Option<tokio::task::JoinHandle<()>> {
    runinator_utilities::liveness::spawn_liveness(
        &config.liveness_file,
        runinator_utilities::liveness::DEFAULT_LIVENESS_INTERVAL,
        shutdown,
    )
}

/// consume wakes, sleep until each is due, then publish a drive on the ingress channel. multiple
/// waker replicas share a consumer group so each wake is handled once; a not-yet-due wake is
/// returned to the broker (nack) after a bounded sleep so the lease never expires under us and
/// other wakes still get serviced. wakes are handled concurrently up to `max_concurrent_wakes`,
/// so one wake sleeping toward its due time never head-of-line blocks a due wake behind it.
pub async fn waker_loop(broker: Arc<dyn Broker>, notify: Arc<Notify>, config: &Config) {
    let group: Arc<str> = Arc::from(config.waker_consumer_group.as_str());
    let max_sleep = Duration::from_secs(config.max_wake_sleep_seconds);
    let slots = Arc::new(Semaphore::new(config.max_concurrent_wakes));
    let mut handlers = JoinSet::new();
    loop {
        // reap finished handlers so the join set does not hold results for the process lifetime.
        while handlers.try_join_next().is_some() {}

        // hold a slot before receiving so this replica never buffers more wakes than it services.
        let slot = tokio::select! {
            _ = notify.notified() => break,
            slot = Arc::clone(&slots).acquire_owned() => match slot {
                Ok(slot) => slot,
                Err(_) => break,
            }
        };
        let delivery = tokio::select! {
            _ = notify.notified() => break,
            received = broker.receive_wake(&group) => {
                match received {
                    Ok(delivery) => delivery,
                    Err(err) => {
                        error!(
                            error_code = error_code_or_unknown(&err),
                            "failed to receive wake: {}", err
                        );
                        // back off so an unreachable broker does not spin this loop hot.
                        tokio::select! {
                            _ = notify.notified() => break,
                            _ = tokio::time::sleep(RECEIVE_RETRY_BACKOFF) => {}
                        }
                        continue;
                    }
                }
            }
        };

        // carries this wake's correlation id through sleep/drive so it can be traced end to end
        // alongside the ws-side ingress/reducer logs that consume the resulting drive.
        let span = tracing::info_span!(
            "wake",
            trace_id = %delivery.command.trace_id,
            run_id = %delivery.command.workflow_run_id,
            node_id = %delivery.command.node_id,
        );
        let broker = Arc::clone(&broker);
        let notify = Arc::clone(&notify);
        let group = Arc::clone(&group);
        handlers.spawn(async move {
            let _slot = slot;
            handle_wake(broker.as_ref(), &group, max_sleep, &notify, delivery)
                .instrument(span)
                .await;
        });
    }
    info!("shutdown signal received, exiting waker loop");
    // drain in-flight handlers; each observes the shutdown notify and nacks its held wake.
    while handlers.join_next().await.is_some() {}
}

async fn handle_wake(
    broker: &dyn Broker,
    group: &str,
    max_sleep: Duration,
    notify: &Notify,
    delivery: runinator_broker::WakeDelivery,
) {
    let now = Utc::now();
    metrics::wake_received((delivery.command.ready_at - now).num_milliseconds() as f64);
    let remaining = (delivery.command.ready_at - now)
        .to_std()
        .unwrap_or_default();

    if remaining.is_zero() {
        drive(broker, group, &delivery).await;
        return;
    }

    let sleep = remaining.min(max_sleep);
    info!(
        sleep_ms = sleep.as_millis() as u64,
        "sleeping until wake is due"
    );
    tokio::select! {
        _ = notify.notified() => {
            let _ = broker.nack_wake(group, delivery.delivery_id).await;
            return;
        }
        _ = tokio::time::sleep(sleep) => {}
    }

    if Utc::now() >= delivery.command.ready_at {
        drive(broker, group, &delivery).await;
    } else {
        metrics::wake_requeued();
        if let Err(err) = broker.nack_wake(group, delivery.delivery_id).await {
            // returning it failed; the broker lease will redeliver it eventually.
            error!(
                error_code = error_code_or_unknown(&err),
                "failed to requeue not-yet-due wake: {}", err
            );
        }
    }
}

async fn drive(broker: &dyn Broker, group: &str, delivery: &runinator_broker::WakeDelivery) {
    let command = WsIngressCommand::drive(
        delivery.command.ready_node_id,
        delivery.command.workflow_run_id,
        delivery.command.node_id.clone(),
        delivery.command.trace_id,
    );
    let message = IngressMessage {
        command,
        dedupe_key: None,
        enqueued_at: Utc::now(),
    };
    // a duplicate means the drive is already in flight; treat it as success and ack the wake.
    match broker.publish_ingress(message).await {
        Ok(()) | Err(runinator_broker::BrokerError::Duplicate(_)) => {
            metrics::wake_driven();
            info!("drive published");
            if let Err(err) = broker.ack_wake(group, delivery.delivery_id).await {
                error!(
                    error_code = error_code_or_unknown(&err),
                    "failed to ack driven wake: {}", err
                );
            }
        }
        Err(err) => {
            metrics::drive_failed();
            error!(
                error_code = error_code_or_unknown(&err),
                "failed to publish drive: {}", err
            );
            if let Err(err) = broker.nack_wake(group, delivery.delivery_id).await {
                error!(
                    error_code = error_code_or_unknown(&err),
                    "failed to requeue wake after drive failure: {}", err
                );
            }
        }
    }
}

#[cfg(test)]
mod tests;
