pub mod config;
pub mod errors;
pub mod metrics;

use std::sync::Arc;
use std::time::Duration;

use chrono::Utc;
use runinator_broker::{Broker, IngressMessage, WsIngressCommand};
use runinator_models::errors::error_code_or_unknown;
use runinator_models::replicas::{ReplicaKind, ReplicaRegistrationRequest};
use runinator_observability::resource_telemetry::{TelemetryCollector, attributes_with_telemetry};
use tokio::sync::{Notify, Semaphore};
use tokio::task::JoinSet;
use tracing::{Instrument, error, info};

use crate::config::Config;

// backoff before retrying a failed wake receive, so a broker outage does not hot-loop the waker.
const RECEIVE_RETRY_BACKOFF: Duration = Duration::from_millis(250);

/// Touch the configured liveness file until shutdown for the Kubernetes exec probe.
/// Returns `None` when no file is configured.
pub fn spawn_liveness(
    config: &Config,
    shutdown: Arc<Notify>,
) -> Option<tokio::task::JoinHandle<()>> {
    runinator_platform::liveness::spawn_liveness(
        &config.liveness_file,
        runinator_platform::liveness::DEFAULT_LIVENESS_INTERVAL,
        shutdown,
    )
}

/// Periodically verify the broker transport while the wake queue is idle.
///
/// This is deliberately a broker protocol heartbeat, not a queued message and not a web-service
/// call. It preserves the timer relay's failure boundary: a heartbeat failure is telemetry and the
/// ordinary receive loop continues retrying the broker until it is reachable again.
pub fn spawn_broker_heartbeat(
    broker: Arc<dyn Broker>,
    config: &Config,
    shutdown: Arc<Notify>,
) -> tokio::task::JoinHandle<()> {
    let interval = Duration::from_secs(config.broker_heartbeat_seconds);
    tokio::spawn(async move {
        let mut ticker = tokio::time::interval(interval);
        ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
        loop {
            tokio::select! {
                _ = shutdown.notified() => return,
                _ = ticker.tick() => {}
            }
            let heartbeat = tokio::select! {
                _ = shutdown.notified() => return,
                result = broker.heartbeat() => result,
            };
            match heartbeat {
                Ok(()) => metrics::broker_heartbeat(),
                Err(err) => {
                    metrics::broker_heartbeat_failed();
                    error!(
                        error_code = error_code_or_unknown(&err),
                        "broker heartbeat failed: {}", err
                    );
                }
            }
        }
    })
}

/// Announce this broker-only waker to the engine. The same ingress lifecycle contract is used by
/// every non-web-service runtime, so the waker never has to reach the web service over HTTP.
pub async fn publish_replica_availability(
    broker: &dyn Broker,
    config: &Config,
    replica_id: uuid::Uuid,
    runtime_id: &str,
    attributes: runinator_models::value::Value,
) -> Result<(), runinator_broker::BrokerError> {
    let registration = ReplicaRegistrationRequest {
        replica_id: Some(replica_id),
        replica_type: ReplicaKind::Waker,
        instance_id: config.waker_id.clone(),
        runtime_id: runtime_id.to_string(),
        display_name: Some(config.waker_id.clone()),
        host: non_blank(&config.advertise_host),
        port: None,
        base_path: None,
        version: Some(env!("CARGO_PKG_VERSION").to_string()),
        attributes,
    };
    let command = WsIngressCommand::replica_available(registration, Vec::new());
    broker
        .publish_ingress(IngressMessage {
            dedupe_key: Some(command.dedupe_key()),
            command,
            enqueued_at: Utc::now(),
        })
        .await
}

/// Send a periodic availability observation and an explicit offline observation on a clean stop.
/// The initial availability is published by the caller before it starts this task, so startup is
/// never silently invisible when the ingress channel is unavailable.
pub fn spawn_replica_heartbeat(
    broker: Arc<dyn Broker>,
    config: Config,
    replica_id: uuid::Uuid,
    runtime_id: String,
    base_attributes: runinator_models::value::Value,
    shutdown: Arc<Notify>,
) -> tokio::task::JoinHandle<()> {
    let telemetry = TelemetryCollector::new();
    tokio::spawn(async move {
        let mut ticker = tokio::time::interval(Duration::from_secs(10));
        loop {
            tokio::select! {
                _ = shutdown.notified() => {
                    let command = WsIngressCommand::replica_offline(replica_id, runtime_id.clone());
                    if let Err(err) = broker.publish_ingress(IngressMessage {
                        dedupe_key: Some(command.dedupe_key()),
                        command,
                        enqueued_at: Utc::now(),
                    }).await {
                        error!(error_code = error_code_or_unknown(&err), "failed to announce waker shutdown: {err}");
                    }
                    return;
                }
                _ = ticker.tick() => {
                    let attributes = attributes_with_telemetry(&base_attributes, &telemetry);
                    if let Err(err) = publish_replica_availability(
                        broker.as_ref(),
                        &config,
                        replica_id,
                        &runtime_id,
                        attributes,
                    ).await {
                        error!(error_code = error_code_or_unknown(&err), "failed to announce waker availability: {err}");
                    }
                }
            }
        }
    })
}

fn non_blank(value: &str) -> Option<String> {
    let value = value.trim();
    (!value.is_empty()).then(|| value.to_string())
}

/// consume wakes, sleep until each is due, then publish the settle on the ingress channel. multiple
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

        // carries this wake's correlation id through the sleep and the settle so it can be traced
        // end to end alongside the engine-side ingress logs that consume the resulting settle.
        let span = tracing::info_span!(
            "wake",
            trace_id = %delivery.command.trace_id,
            run_id = %delivery.command.workflow_run_id(),
            effect_id = %delivery.command.effect_id(),
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
    metrics::wake_received((delivery.command.due_at - now).num_milliseconds() as f64);
    let remaining = (delivery.command.due_at - now).to_std().unwrap_or_default();
    runinator_observability::tui::activity(
        "waker",
        format!("wake {}", delivery.command.effect_id()),
        (!remaining.is_zero()).then_some(remaining),
    );

    if remaining.is_zero() {
        settle(broker, group, &delivery).await;
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

    if Utc::now() >= delivery.command.due_at {
        settle(broker, group, &delivery).await;
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

/// hand the wake's carried result back to the engine on the ingress channel.
///
/// the result is relayed verbatim: it was built (and its timestamp stamped at `due_at`) by the
/// infrastructure effect host that armed this timer, so the waker never needs to know what kind of
/// effect it is settling.
async fn settle(broker: &dyn Broker, group: &str, delivery: &runinator_broker::WakeDelivery) {
    runinator_observability::tui::activity(
        "waker",
        format!("settling wake {}", delivery.command.effect_id()),
        None,
    );
    metrics::wake_due_lag(
        (Utc::now() - delivery.command.due_at)
            .num_milliseconds()
            .max(0) as f64,
    );
    let command = if let Some(wake) = &delivery.command.orchestration_intent {
        WsIngressCommand::orchestration_intent(
            wake.clone(),
            delivery.command.due_at,
            delivery.command.trace_id,
        )
    } else if let Some(timer) = &delivery.command.timer_interrupt {
        WsIngressCommand::timer_interrupt(
            timer.clone(),
            delivery.command.due_at,
            delivery.command.trace_id,
        )
    } else {
        WsIngressCommand::settle_effect(delivery.command.result.clone(), delivery.command.trace_id)
    };
    let dedupe_key = delivery
        .command
        .orchestration_intent
        .as_ref()
        .map(|_| delivery.command.dedupe_key());
    let message = IngressMessage {
        command,
        dedupe_key,
        enqueued_at: Utc::now(),
    };
    // a duplicate means the settle is already in flight; treat it as success and ack the wake.
    match broker.publish_ingress(message).await {
        Ok(()) | Err(runinator_broker::BrokerError::Duplicate(_)) => {
            metrics::wake_driven();
            info!("settle published");
            if let Err(err) = broker.ack_wake(group, delivery.delivery_id).await {
                error!(
                    error_code = error_code_or_unknown(&err),
                    "failed to ack settled wake: {}", err
                );
            }
        }
        Err(err) => {
            metrics::drive_failed();
            error!(
                error_code = error_code_or_unknown(&err),
                "failed to publish settle: {}", err
            );
            if let Err(err) = broker.nack_wake(group, delivery.delivery_id).await {
                error!(
                    error_code = error_code_or_unknown(&err),
                    "failed to requeue wake after settle failure: {}", err
                );
            }
        }
    }
}

#[cfg(test)]
mod tests;
