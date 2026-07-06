pub mod config;
pub mod errors;
pub mod metrics;

use std::sync::Arc;
use std::time::Duration;

use chrono::Utc;
use runinator_broker::{Broker, IngressMessage, WsIngressCommand};
use runinator_models::errors::error_code_or_unknown;
use tokio::sync::Notify;
use tracing::{Instrument, error, info};

use crate::config::Config;

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
/// other wakes still get serviced.
pub async fn waker_loop(broker: Arc<dyn Broker>, notify: Arc<Notify>, config: &Config) {
    let group = config.waker_consumer_group.as_str();
    let max_sleep = Duration::from_secs(config.max_wake_sleep_seconds);
    loop {
        let delivery = tokio::select! {
            _ = notify.notified() => {
                info!("shutdown signal received, exiting waker loop");
                return;
            }
            received = broker.receive_wake(group) => {
                match received {
                    Ok(delivery) => delivery,
                    Err(err) => {
                        error!(
                            error_code = error_code_or_unknown(&err),
                            "failed to receive wake: {}", err
                        );
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
        let outcome = handle_wake(broker.as_ref(), group, max_sleep, &notify, delivery, config)
            .instrument(span)
            .await;
        if outcome == WakeOutcome::Shutdown {
            info!("shutdown signal received, exiting waker loop");
            return;
        }
    }
}

#[derive(PartialEq, Eq)]
enum WakeOutcome {
    Continue,
    Shutdown,
}

async fn handle_wake(
    broker: &dyn Broker,
    group: &str,
    max_sleep: Duration,
    notify: &Notify,
    delivery: runinator_broker::WakeDelivery,
    config: &Config,
) -> WakeOutcome {
    let now = Utc::now();
    metrics::wake_received((delivery.command.ready_at - now).num_milliseconds() as f64);
    let remaining = (delivery.command.ready_at - now)
        .to_std()
        .unwrap_or_default();

    if remaining.is_zero() {
        drive(broker, group, &delivery, config).await;
        return WakeOutcome::Continue;
    }

    let sleep = remaining.min(max_sleep);
    info!(
        sleep_ms = sleep.as_millis() as u64,
        "sleeping until wake is due"
    );
    tokio::select! {
        _ = notify.notified() => {
            let _ = broker.nack_wake(group, delivery.delivery_id).await;
            return WakeOutcome::Shutdown;
        }
        _ = tokio::time::sleep(sleep) => {}
    }

    if Utc::now() >= delivery.command.ready_at {
        drive(broker, group, &delivery, config).await;
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
    WakeOutcome::Continue
}

async fn drive(
    broker: &dyn Broker,
    group: &str,
    delivery: &runinator_broker::WakeDelivery,
    _config: &Config,
) {
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
