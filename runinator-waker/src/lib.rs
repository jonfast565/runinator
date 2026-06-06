pub mod config;
pub mod errors;

use std::sync::Arc;
use std::time::Duration;

use chrono::Utc;
use log::{error, info};
use runinator_broker::{Broker, IngressMessage, WsIngressCommand};
use tokio::sync::Notify;

use crate::config::Config;

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
                info!("Shutdown signal received. Exiting waker loop.");
                return;
            }
            received = broker.receive_wake(group) => {
                match received {
                    Ok(delivery) => delivery,
                    Err(err) => {
                        error!("Failed to receive wake: {}", err);
                        continue;
                    }
                }
            }
        };

        let remaining = (delivery.command.ready_at - Utc::now())
            .to_std()
            .unwrap_or_default();

        if remaining.is_zero() {
            drive(broker.as_ref(), group, &delivery, config).await;
            continue;
        }

        let sleep = remaining.min(max_sleep);
        tokio::select! {
            _ = notify.notified() => {
                let _ = broker.nack_wake(group, delivery.delivery_id).await;
                info!("Shutdown signal received. Exiting waker loop.");
                return;
            }
            _ = tokio::time::sleep(sleep) => {}
        }

        if Utc::now() >= delivery.command.ready_at {
            drive(broker.as_ref(), group, &delivery, config).await;
        } else if let Err(err) = broker.nack_wake(group, delivery.delivery_id).await {
            // returning it failed; the broker lease will redeliver it eventually.
            error!("Failed to requeue not-yet-due wake: {}", err);
        }
    }
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
    );
    let message = IngressMessage {
        command,
        dedupe_key: None,
        enqueued_at: Utc::now(),
    };
    // a duplicate means the drive is already in flight; treat it as success and ack the wake.
    match broker.publish_ingress(message).await {
        Ok(()) | Err(runinator_broker::BrokerError::Duplicate(_)) => {
            if let Err(err) = broker.ack_wake(group, delivery.delivery_id).await {
                error!("Failed to ack driven wake: {}", err);
            }
        }
        Err(err) => {
            error!(
                "Failed to publish drive for ready node {}: {}",
                delivery.command.ready_node_id, err
            );
            if let Err(err) = broker.nack_wake(group, delivery.delivery_id).await {
                error!("Failed to requeue wake after drive failure: {}", err);
            }
        }
    }
}
