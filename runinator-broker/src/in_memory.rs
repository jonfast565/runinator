use crate::{
    Broker, BrokerDelivery, BrokerError, BrokerMessage, ConsumerProfile, ControlCommand,
    ControlDelivery, EventDelivery, EventMessage, IngressDelivery, IngressMessage, ResultDelivery,
    ResultMessage, WakeDelivery, WakeMessage,
};
use async_trait::async_trait;
use parking_lot::Mutex;
use std::collections::{HashMap, HashSet, VecDeque};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::{broadcast, Mutex as AsyncMutex, Notify};
use uuid::Uuid;

const EVENT_CHANNEL_CAPACITY: usize = 1024;

#[derive(Default)]
struct BrokerState {
    queue: VecDeque<BrokerDelivery>,
    inflight: HashMap<Uuid, Leased<BrokerDelivery>>,
    dedupe: HashSet<String>,
    control_queue: VecDeque<ControlDelivery>,
    control_inflight: HashMap<Uuid, Leased<ControlDelivery>>,
    result_queue: VecDeque<ResultDelivery>,
    result_inflight: HashMap<Uuid, Leased<ResultDelivery>>,
    result_dedupe: HashSet<String>,
    wake_queue: VecDeque<WakeDelivery>,
    wake_inflight: HashMap<Uuid, Leased<WakeDelivery>>,
    wake_dedupe: HashSet<String>,
    ingress_queue: VecDeque<IngressDelivery>,
    ingress_inflight: HashMap<Uuid, Leased<IngressDelivery>>,
    ingress_dedupe: HashSet<String>,
}

struct Leased<T> {
    delivery: T,
    leased_until: Instant,
}

type EventReceiver = Arc<AsyncMutex<broadcast::Receiver<EventDelivery>>>;

#[derive(Clone)]
pub struct InMemoryBroker {
    state: Arc<Mutex<BrokerState>>,
    notify: Arc<Notify>,
    control_notify: Arc<Notify>,
    result_notify: Arc<Notify>,
    wake_notify: Arc<Notify>,
    ingress_notify: Arc<Notify>,
    // fan-out: every subscriber drains its own receiver of every published event.
    event_tx: broadcast::Sender<EventDelivery>,
    event_subscribers: Arc<Mutex<HashMap<String, EventReceiver>>>,
    lease_duration: Duration,
}

impl InMemoryBroker {
    const DEFAULT_LEASE_DURATION: Duration = Duration::from_secs(30);

    pub fn new() -> Self {
        Self::default()
    }

    pub fn with_lease_duration(lease_duration: Duration) -> Self {
        Self {
            lease_duration,
            ..Self::default()
        }
    }
}

impl Default for InMemoryBroker {
    fn default() -> Self {
        let (event_tx, _) = broadcast::channel(EVENT_CHANNEL_CAPACITY);
        Self {
            state: Arc::new(Mutex::new(BrokerState::default())),
            notify: Arc::new(Notify::new()),
            control_notify: Arc::new(Notify::new()),
            result_notify: Arc::new(Notify::new()),
            wake_notify: Arc::new(Notify::new()),
            ingress_notify: Arc::new(Notify::new()),
            event_tx,
            event_subscribers: Arc::new(Mutex::new(HashMap::new())),
            lease_duration: Self::DEFAULT_LEASE_DURATION,
        }
    }
}

impl InMemoryBroker {
    /// wait for and lease the first queued control delivery accepted by `matches`. targeted scan:
    /// a non-matching head must not block controls for other consumers queued behind it, and a
    /// control targeted at a consumer that never returns is dropped once it goes stale.
    async fn receive_control_matching(
        &self,
        matches: impl Fn(&ControlDelivery) -> bool,
    ) -> Result<ControlDelivery, BrokerError> {
        loop {
            // register for wakeups before scanning: control publishes use notify_waiters (no
            // stored permit), so a publish landing between the scan and the wait would otherwise
            // be lost until the sleep fallback fires.
            let notified = self.control_notify.notified();
            tokio::pin!(notified);
            notified.as_mut().enable();
            if let Some(delivery) = {
                let mut guard = self.state.lock();
                reclaim_expired_control(&mut guard, Instant::now());
                drop_stale_control(&mut guard, chrono::Utc::now());
                let index = guard.control_queue.iter().position(&matches);
                match index.and_then(|index| guard.control_queue.remove(index)) {
                    Some(delivery) => {
                        guard.control_inflight.insert(
                            delivery.delivery_id,
                            Leased {
                                delivery: delivery.clone(),
                                leased_until: Instant::now() + self.lease_duration,
                            },
                        );
                        Some(delivery)
                    }
                    None => None,
                }
            } {
                return Ok(delivery);
            }

            tokio::select! {
                _ = &mut notified => {}
                _ = tokio::time::sleep(self.lease_duration) => {}
            }
        }
    }

    /// get-or-create the dedicated fan-out receiver for one subscriber id.
    fn event_receiver(&self, consumer: &str) -> EventReceiver {
        let mut guard = self.event_subscribers.lock();
        if let Some(rx) = guard.get(consumer) {
            return Arc::clone(rx);
        }
        let rx = Arc::new(AsyncMutex::new(self.event_tx.subscribe()));
        guard.insert(consumer.to_string(), Arc::clone(&rx));
        rx
    }
}

#[async_trait]
impl Broker for InMemoryBroker {
    fn supports_workflow_result_channels(&self) -> bool {
        true
    }

    async fn publish(&self, message: BrokerMessage) -> Result<(), BrokerError> {
        let mut guard = self.state.lock();
        let dedupe = message.dedupe_key_or_hash();
        if !guard.dedupe.insert(dedupe.clone()) {
            return Err(BrokerError::Duplicate(dedupe));
        }

        let delivery: BrokerDelivery = message.into();
        guard.queue.push_back(delivery);
        drop(guard);
        // deliveries are targeted, so wake every waiter: notify_one could wake a consumer whose
        // profile does not match, leaving the matching consumer asleep for a full lease period.
        self.notify.notify_waiters();
        Ok(())
    }

    async fn receive(&self, consumer: &str) -> Result<BrokerDelivery, BrokerError> {
        // a plain consumer is a general-pool consumer; it must not pick up replica/label-targeted
        // deliveries intended for a specific worker.
        self.receive_for(&ConsumerProfile::shared(consumer)).await
    }

    async fn receive_for(&self, profile: &ConsumerProfile) -> Result<BrokerDelivery, BrokerError> {
        loop {
            // register for wakeups before scanning: publishes use notify_waiters (no stored
            // permit), so a publish landing between the scan and the wait would otherwise be lost
            // until the sleep fallback fires.
            let notified = self.notify.notified();
            tokio::pin!(notified);
            notified.as_mut().enable();
            if let Some(delivery) = {
                let mut guard = self.state.lock();
                reclaim_expired_actions(&mut guard, Instant::now());
                // scan for the first delivery whose target matches this consumer. a non-matching
                // head must not block matching deliveries queued behind it.
                let index = guard
                    .queue
                    .iter()
                    .position(|delivery| delivery.command.target.matches(profile));
                match index.and_then(|index| guard.queue.remove(index)) {
                    Some(delivery) => {
                        guard.inflight.insert(
                            delivery.delivery_id,
                            Leased {
                                delivery: delivery.clone(),
                                leased_until: Instant::now() + self.lease_duration,
                            },
                        );
                        Some(delivery)
                    }
                    None => None,
                }
            } {
                return Ok(delivery);
            }

            tokio::select! {
                _ = &mut notified => {}
                _ = tokio::time::sleep(self.lease_duration) => {}
            }
        }
    }

    async fn ack(&self, _consumer: &str, delivery_id: Uuid) -> Result<(), BrokerError> {
        let mut guard = self.state.lock();
        if let Some(leased) = guard.inflight.remove(&delivery_id) {
            guard.dedupe.remove(&leased.delivery.dedupe_key);
            Ok(())
        } else {
            Err(BrokerError::UnknownDelivery(delivery_id))
        }
    }

    async fn nack(&self, _consumer: &str, delivery_id: Uuid) -> Result<(), BrokerError> {
        let mut guard = self.state.lock();
        if let Some(leased) = guard.inflight.remove(&delivery_id) {
            guard.queue.push_front(redeliver_action(leased.delivery));
            drop(guard);
            // wake sleeping consumers so a requeued delivery is not stranded until the sleep
            // fallback when the nacking consumer disconnects right after returning it.
            self.notify.notify_waiters();
            Ok(())
        } else {
            Err(BrokerError::UnknownDelivery(delivery_id))
        }
    }

    async fn publish_control(&self, command: ControlCommand) -> Result<(), BrokerError> {
        let mut guard = self.state.lock();
        guard.control_queue.push_back(command.into());
        drop(guard);
        // controls are targeted, so wake every waiter: notify_one could wake a consumer whose
        // profile does not match, leaving the matching consumer asleep for a full lease period.
        self.control_notify.notify_waiters();
        Ok(())
    }

    async fn receive_control(&self, _consumer: &str) -> Result<ControlDelivery, BrokerError> {
        // the legacy untargeted path: hand over the head of the queue regardless of target.
        self.receive_control_matching(|_| true).await
    }

    async fn receive_control_for(
        &self,
        profile: &ConsumerProfile,
    ) -> Result<ControlDelivery, BrokerError> {
        self.receive_control_matching(|delivery| delivery.command.target.matches(profile))
            .await
    }

    async fn ack_control(&self, _consumer: &str, delivery_id: Uuid) -> Result<(), BrokerError> {
        let mut guard = self.state.lock();
        if guard.control_inflight.remove(&delivery_id).is_some() {
            Ok(())
        } else {
            Err(BrokerError::UnknownDelivery(delivery_id))
        }
    }

    async fn nack_control(&self, _consumer: &str, delivery_id: Uuid) -> Result<(), BrokerError> {
        let mut guard = self.state.lock();
        if let Some(leased) = guard.control_inflight.remove(&delivery_id) {
            guard
                .control_queue
                .push_front(redeliver_control(leased.delivery));
            drop(guard);
            self.control_notify.notify_waiters();
            Ok(())
        } else {
            Err(BrokerError::UnknownDelivery(delivery_id))
        }
    }

    async fn publish_result(&self, message: ResultMessage) -> Result<(), BrokerError> {
        let mut guard = self.state.lock();
        let dedupe = message.dedupe_key_or_hash();
        if !guard.result_dedupe.insert(dedupe.clone()) {
            return Err(BrokerError::Duplicate(dedupe));
        }

        let delivery: ResultDelivery = message.into();
        guard.result_queue.push_back(delivery);
        drop(guard);
        self.result_notify.notify_one();
        Ok(())
    }

    async fn receive_result(&self, _consumer: &str) -> Result<ResultDelivery, BrokerError> {
        loop {
            if let Some(delivery) = {
                let mut guard = self.state.lock();
                reclaim_expired_results(&mut guard, Instant::now());
                if let Some(delivery) = guard.result_queue.pop_front() {
                    guard.result_inflight.insert(
                        delivery.delivery_id,
                        Leased {
                            delivery: delivery.clone(),
                            leased_until: Instant::now() + self.lease_duration,
                        },
                    );
                    Some(delivery)
                } else {
                    None
                }
            } {
                return Ok(delivery);
            }

            tokio::select! {
                _ = self.result_notify.notified() => {}
                _ = tokio::time::sleep(self.lease_duration) => {}
            }
        }
    }

    async fn ack_result(&self, _consumer: &str, delivery_id: Uuid) -> Result<(), BrokerError> {
        let mut guard = self.state.lock();
        if let Some(leased) = guard.result_inflight.remove(&delivery_id) {
            guard.result_dedupe.remove(&leased.delivery.dedupe_key);
            Ok(())
        } else {
            Err(BrokerError::UnknownDelivery(delivery_id))
        }
    }

    async fn nack_result(&self, _consumer: &str, delivery_id: Uuid) -> Result<(), BrokerError> {
        let mut guard = self.state.lock();
        if let Some(leased) = guard.result_inflight.remove(&delivery_id) {
            guard
                .result_queue
                .push_front(redeliver_result(leased.delivery));
            drop(guard);
            self.result_notify.notify_one();
            Ok(())
        } else {
            Err(BrokerError::UnknownDelivery(delivery_id))
        }
    }

    async fn publish_wake(&self, message: WakeMessage) -> Result<(), BrokerError> {
        let mut guard = self.state.lock();
        let dedupe = message.dedupe_key_or_hash();
        if !guard.wake_dedupe.insert(dedupe.clone()) {
            return Err(BrokerError::Duplicate(dedupe));
        }

        let delivery: WakeDelivery = message.into();
        guard.wake_queue.push_back(delivery);
        drop(guard);
        self.wake_notify.notify_one();
        Ok(())
    }

    async fn receive_wake(&self, _consumer: &str) -> Result<WakeDelivery, BrokerError> {
        loop {
            if let Some(delivery) = {
                let mut guard = self.state.lock();
                reclaim_expired_wakes(&mut guard, Instant::now());
                if let Some(delivery) = guard.wake_queue.pop_front() {
                    guard.wake_inflight.insert(
                        delivery.delivery_id,
                        Leased {
                            delivery: delivery.clone(),
                            leased_until: Instant::now() + self.lease_duration,
                        },
                    );
                    Some(delivery)
                } else {
                    None
                }
            } {
                return Ok(delivery);
            }

            tokio::select! {
                _ = self.wake_notify.notified() => {}
                _ = tokio::time::sleep(self.lease_duration) => {}
            }
        }
    }

    async fn ack_wake(&self, _consumer: &str, delivery_id: Uuid) -> Result<(), BrokerError> {
        let mut guard = self.state.lock();
        if let Some(leased) = guard.wake_inflight.remove(&delivery_id) {
            guard.wake_dedupe.remove(&leased.delivery.dedupe_key);
            Ok(())
        } else {
            Err(BrokerError::UnknownDelivery(delivery_id))
        }
    }

    async fn nack_wake(&self, _consumer: &str, delivery_id: Uuid) -> Result<(), BrokerError> {
        let mut guard = self.state.lock();
        if let Some(leased) = guard.wake_inflight.remove(&delivery_id) {
            guard.wake_queue.push_front(redeliver_wake(leased.delivery));
            drop(guard);
            self.wake_notify.notify_one();
            Ok(())
        } else {
            Err(BrokerError::UnknownDelivery(delivery_id))
        }
    }

    async fn publish_ingress(&self, message: IngressMessage) -> Result<(), BrokerError> {
        let mut guard = self.state.lock();
        let dedupe = message.dedupe_key_or_hash();
        if !guard.ingress_dedupe.insert(dedupe.clone()) {
            return Err(BrokerError::Duplicate(dedupe));
        }

        let delivery: IngressDelivery = message.into();
        guard.ingress_queue.push_back(delivery);
        drop(guard);
        self.ingress_notify.notify_one();
        Ok(())
    }

    async fn receive_ingress(&self, _consumer: &str) -> Result<IngressDelivery, BrokerError> {
        loop {
            if let Some(delivery) = {
                let mut guard = self.state.lock();
                reclaim_expired_ingress(&mut guard, Instant::now());
                if let Some(delivery) = guard.ingress_queue.pop_front() {
                    guard.ingress_inflight.insert(
                        delivery.delivery_id,
                        Leased {
                            delivery: delivery.clone(),
                            leased_until: Instant::now() + self.lease_duration,
                        },
                    );
                    Some(delivery)
                } else {
                    None
                }
            } {
                return Ok(delivery);
            }

            tokio::select! {
                _ = self.ingress_notify.notified() => {}
                _ = tokio::time::sleep(self.lease_duration) => {}
            }
        }
    }

    async fn ack_ingress(&self, _consumer: &str, delivery_id: Uuid) -> Result<(), BrokerError> {
        let mut guard = self.state.lock();
        if let Some(leased) = guard.ingress_inflight.remove(&delivery_id) {
            guard.ingress_dedupe.remove(&leased.delivery.dedupe_key);
            Ok(())
        } else {
            Err(BrokerError::UnknownDelivery(delivery_id))
        }
    }

    async fn nack_ingress(&self, _consumer: &str, delivery_id: Uuid) -> Result<(), BrokerError> {
        let mut guard = self.state.lock();
        if let Some(leased) = guard.ingress_inflight.remove(&delivery_id) {
            guard
                .ingress_queue
                .push_front(redeliver_ingress(leased.delivery));
            drop(guard);
            self.ingress_notify.notify_one();
            Ok(())
        } else {
            Err(BrokerError::UnknownDelivery(delivery_id))
        }
    }

    async fn publish_event(&self, message: EventMessage) -> Result<(), BrokerError> {
        // fan-out, best-effort: no subscribers is not an error.
        let _ = self.event_tx.send(message.into());
        Ok(())
    }

    async fn receive_event(&self, consumer: &str) -> Result<EventDelivery, BrokerError> {
        let receiver = self.event_receiver(consumer);
        let mut guard = receiver.lock().await;
        loop {
            match guard.recv().await {
                Ok(delivery) => return Ok(delivery),
                // a slow subscriber that lagged behind just resumes from the newest events.
                Err(broadcast::error::RecvError::Lagged(_)) => continue,
                Err(broadcast::error::RecvError::Closed) => {
                    return Err(BrokerError::Internal("event channel closed".into()));
                }
            }
        }
    }
}

fn reclaim_expired_actions(state: &mut BrokerState, now: Instant) {
    let expired = expired_ids(&state.inflight, now);
    for id in expired {
        if let Some(leased) = state.inflight.remove(&id) {
            state.queue.push_front(redeliver_action(leased.delivery));
        }
    }
}

fn reclaim_expired_control(state: &mut BrokerState, now: Instant) {
    let expired = expired_ids(&state.control_inflight, now);
    for id in expired {
        if let Some(leased) = state.control_inflight.remove(&id) {
            state
                .control_queue
                .push_front(redeliver_control(leased.delivery));
        }
    }
}

/// drop queued controls that have gone stale: a control targeted at a replica that never returns
/// has no consumer that can ever match it, and controls are immediate signals, so retaining one
/// past the ttl only grows the queue (this broker also backs the long-lived http/tcp servers).
fn drop_stale_control(state: &mut BrokerState, now: chrono::DateTime<chrono::Utc>) {
    state.control_queue.retain(|delivery| {
        (now - delivery.enqueued_at).num_seconds() < crate::STALE_CONTROL_TTL_SECONDS
    });
}

fn reclaim_expired_results(state: &mut BrokerState, now: Instant) {
    let expired = expired_ids(&state.result_inflight, now);
    for id in expired {
        if let Some(leased) = state.result_inflight.remove(&id) {
            state
                .result_queue
                .push_front(redeliver_result(leased.delivery));
        }
    }
}

fn reclaim_expired_wakes(state: &mut BrokerState, now: Instant) {
    let expired = expired_ids(&state.wake_inflight, now);
    for id in expired {
        if let Some(leased) = state.wake_inflight.remove(&id) {
            state.wake_queue.push_front(redeliver_wake(leased.delivery));
        }
    }
}

fn reclaim_expired_ingress(state: &mut BrokerState, now: Instant) {
    let expired = expired_ids(&state.ingress_inflight, now);
    for id in expired {
        if let Some(leased) = state.ingress_inflight.remove(&id) {
            state
                .ingress_queue
                .push_front(redeliver_ingress(leased.delivery));
        }
    }
}

fn expired_ids<T>(inflight: &HashMap<Uuid, Leased<T>>, now: Instant) -> Vec<Uuid> {
    inflight
        .iter()
        .filter_map(|(id, leased)| (leased.leased_until <= now).then_some(*id))
        .collect()
}

fn redeliver_action(delivery: BrokerDelivery) -> BrokerDelivery {
    BrokerDelivery {
        delivery_id: Uuid::new_v4(),
        ..delivery
    }
}

fn redeliver_control(delivery: ControlDelivery) -> ControlDelivery {
    ControlDelivery {
        delivery_id: Uuid::new_v4(),
        ..delivery
    }
}

fn redeliver_result(delivery: ResultDelivery) -> ResultDelivery {
    ResultDelivery {
        delivery_id: Uuid::new_v4(),
        ..delivery
    }
}

fn redeliver_wake(delivery: WakeDelivery) -> WakeDelivery {
    WakeDelivery {
        delivery_id: Uuid::new_v4(),
        ..delivery
    }
}

fn redeliver_ingress(delivery: IngressDelivery) -> IngressDelivery {
    IngressDelivery {
        delivery_id: Uuid::new_v4(),
        ..delivery
    }
}

#[cfg(test)]
mod tests {
    use chrono::Utc;
    use runinator_comm::ActionCommand;
    use runinator_models::json;
    use runinator_models::workflows::WorkflowAction;

    use crate::{Broker, BrokerMessage, ResultMessage};

    use super::*;

    #[test]
    fn in_memory_broker_supports_workflow_result_channels() {
        assert!(InMemoryBroker::new().supports_workflow_result_channels());
    }

    #[tokio::test]
    async fn in_memory_broker_redelivers_expired_action_delivery() {
        let broker = InMemoryBroker::with_lease_duration(Duration::from_millis(10));
        broker
            .publish(BrokerMessage {
                command: action_command(),
                dedupe_key: Some("lease-action".into()),
                enqueued_at: Utc::now(),
            })
            .await
            .unwrap();

        let first = broker.receive("consumer-a").await.unwrap();
        tokio::time::sleep(Duration::from_millis(15)).await;
        let second = broker.receive("consumer-b").await.unwrap();

        assert_ne!(first.delivery_id, second.delivery_id);
        assert_eq!(first.command.command_id, second.command.command_id);
        assert!(broker.ack("consumer-a", first.delivery_id).await.is_err());
        broker.ack("consumer-b", second.delivery_id).await.unwrap();
    }

    #[tokio::test]
    async fn in_memory_broker_redelivers_expired_result_delivery() {
        let broker = InMemoryBroker::with_lease_duration(Duration::from_millis(10));
        let command = action_command();
        let event = runinator_comm::WorkflowResultEvent::status(
            &command,
            runinator_models::workflows::WorkflowStatus::Succeeded,
            None,
            None,
        );
        broker
            .publish_result(ResultMessage {
                event,
                dedupe_key: Some("lease-result".into()),
                enqueued_at: Utc::now(),
            })
            .await
            .unwrap();

        let first = broker.receive_result("consumer-a").await.unwrap();
        tokio::time::sleep(Duration::from_millis(15)).await;
        let second = broker.receive_result("consumer-b").await.unwrap();

        assert_ne!(first.delivery_id, second.delivery_id);
        assert_eq!(first.event.event_id, second.event.event_id);
        assert!(broker
            .ack_result("consumer-a", first.delivery_id)
            .await
            .is_err());
        broker
            .ack_result("consumer-b", second.delivery_id)
            .await
            .unwrap();
    }

    #[tokio::test]
    async fn in_memory_broker_redelivers_expired_wake_delivery() {
        let broker = InMemoryBroker::with_lease_duration(Duration::from_millis(10));
        let command = runinator_comm::WakeCommand::new(
            Uuid::now_v7(),
            Uuid::now_v7(),
            "node-a".into(),
            Utc::now(),
            Uuid::new_v4(),
            Uuid::now_v7(),
        );
        broker
            .publish_wake(crate::WakeMessage {
                command,
                dedupe_key: Some("lease-wake".into()),
                enqueued_at: Utc::now(),
            })
            .await
            .unwrap();

        let first = broker.receive_wake("consumer-a").await.unwrap();
        tokio::time::sleep(Duration::from_millis(15)).await;
        let second = broker.receive_wake("consumer-b").await.unwrap();

        assert_ne!(first.delivery_id, second.delivery_id);
        assert_eq!(first.command.ready_node_id, second.command.ready_node_id);
        assert!(broker
            .ack_wake("consumer-a", first.delivery_id)
            .await
            .is_err());
        broker
            .ack_wake("consumer-b", second.delivery_id)
            .await
            .unwrap();
    }

    #[tokio::test]
    async fn in_memory_broker_round_trips_ingress_delivery() {
        let broker = InMemoryBroker::new();
        let command = runinator_comm::WsIngressCommand::drive(
            Uuid::now_v7(),
            Uuid::now_v7(),
            "node-a".into(),
            Uuid::now_v7(),
        );
        broker
            .publish_ingress(crate::IngressMessage {
                command,
                dedupe_key: None,
                enqueued_at: Utc::now(),
            })
            .await
            .unwrap();

        let delivery = broker.receive_ingress("ws").await.unwrap();
        assert!(matches!(
            delivery.command,
            runinator_comm::WsIngressCommand::Drive { .. }
        ));
        broker
            .ack_ingress("ws", delivery.delivery_id)
            .await
            .unwrap();
    }

    #[tokio::test]
    async fn in_memory_broker_fans_out_events_to_every_subscriber() {
        use crate::EventMessage;
        use runinator_comm::UiEvent;

        let broker = InMemoryBroker::new();
        // both subscribers must register before publishing so each gets the event.
        let _ = broker.event_receiver("ws-a");
        let _ = broker.event_receiver("ws-b");

        broker
            .publish_event(EventMessage::new(UiEvent::WorkflowsChanged))
            .await
            .unwrap();

        let a = broker.receive_event("ws-a").await.unwrap();
        let b = broker.receive_event("ws-b").await.unwrap();
        assert!(matches!(a.event, UiEvent::WorkflowsChanged));
        assert!(matches!(b.event, UiEvent::WorkflowsChanged));
    }

    #[tokio::test]
    async fn receive_control_for_routes_targeted_controls_to_the_matching_replica() {
        use runinator_comm::{ConsumerProfile, ControlCommand, ControlKind};

        let broker = InMemoryBroker::new();
        let holder = Uuid::now_v7();
        let bystander = Uuid::now_v7();
        let run_id = Uuid::now_v7();

        // a cancel routed to the executor-holding replica, queued behind nothing special.
        let targeted = ControlCommand::for_node_run(run_id, Uuid::now_v7(), ControlKind::Cancel)
            .targeting_replica(holder);
        broker.publish_control(targeted).await.unwrap();

        // a worker that is not the holder must never receive it, even when polling first; the
        // 50ms timeout bounds the test rather than proving absence forever.
        let bystander_profile = ConsumerProfile::shared("worker-b").with_replica_id(bystander);
        let unmatched = tokio::time::timeout(
            Duration::from_millis(50),
            broker.receive_control_for(&bystander_profile),
        )
        .await;
        assert!(unmatched.is_err(), "targeted control leaked to a bystander");

        // the holder receives it.
        let holder_profile = ConsumerProfile::shared("worker-a").with_replica_id(holder);
        let delivery = broker.receive_control_for(&holder_profile).await.unwrap();
        assert_eq!(delivery.command.workflow_run_id, run_id);
        broker
            .ack_control("worker-a", delivery.delivery_id)
            .await
            .unwrap();
    }

    #[tokio::test]
    async fn receive_control_for_hands_untargeted_controls_to_any_non_exclusive_profile() {
        use runinator_comm::{ConsumerProfile, ControlCommand, ControlKind};

        let broker = InMemoryBroker::new();
        let run_id = Uuid::now_v7();
        broker
            .publish_control(ControlCommand::new(run_id, ControlKind::Cancel))
            .await
            .unwrap();

        // the worker's control profile is never exclusive, so a run-wide `Any` control matches a
        // replica-bound profile too (a desktop worker must still see run-wide cancels).
        let profile = ConsumerProfile::shared("desktop").with_replica_id(Uuid::now_v7());
        let delivery = broker.receive_control_for(&profile).await.unwrap();
        assert_eq!(delivery.command.workflow_run_id, run_id);
    }

    #[tokio::test]
    async fn stale_queued_controls_are_dropped_not_delivered() {
        use runinator_comm::{ControlCommand, ControlKind};

        let mut state = BrokerState::default();
        let delivery: ControlDelivery =
            ControlCommand::new(Uuid::now_v7(), ControlKind::Cancel).into();
        state.control_queue.push_back(delivery);

        // fresh controls survive the sweep; one past the ttl is dropped.
        drop_stale_control(&mut state, chrono::Utc::now());
        assert_eq!(state.control_queue.len(), 1);
        let past_ttl =
            chrono::Utc::now() + chrono::Duration::seconds(crate::STALE_CONTROL_TTL_SECONDS + 1);
        drop_stale_control(&mut state, past_ttl);
        assert!(state.control_queue.is_empty());
    }

    #[tokio::test]
    async fn nack_control_requeues_for_the_matching_consumer() {
        use runinator_comm::{ConsumerProfile, ControlCommand, ControlKind};

        let broker = InMemoryBroker::new();
        let run_id = Uuid::now_v7();
        broker
            .publish_control(ControlCommand::new(run_id, ControlKind::Pause))
            .await
            .unwrap();

        // the legacy untargeted path takes it; a nack must return it for redelivery.
        let first = broker.receive_control("worker-a").await.unwrap();
        broker
            .nack_control("worker-a", first.delivery_id)
            .await
            .unwrap();
        let second = broker
            .receive_control_for(&ConsumerProfile::shared("worker-b"))
            .await
            .unwrap();
        assert_eq!(second.command.workflow_run_id, run_id);
        assert_ne!(first.delivery_id, second.delivery_id);
    }

    #[tokio::test]
    async fn receive_for_routes_targeted_actions_to_the_matching_consumer() {
        use runinator_comm::{ActionTarget, ConsumerProfile};

        let broker = InMemoryBroker::new();
        let replica = Uuid::now_v7();

        // a replica-targeted action and a general-pool (Any) action share the queue.
        let mut targeted = action_command();
        targeted.target = ActionTarget::Replica {
            replica_id: replica,
        };
        let any = action_command();
        broker
            .publish(BrokerMessage {
                command: targeted.clone(),
                dedupe_key: Some("targeted".into()),
                enqueued_at: Utc::now(),
            })
            .await
            .unwrap();
        broker
            .publish(BrokerMessage {
                command: any.clone(),
                dedupe_key: Some("any".into()),
                enqueued_at: Utc::now(),
            })
            .await
            .unwrap();

        // an exclusive consumer bound to the replica only sees the targeted action, even though it
        // sits ahead of nothing special; it must never receive the Any action.
        let desktop = ConsumerProfile::shared("desktop")
            .with_replica_id(replica)
            .exclusive();
        let delivery = broker.receive_for(&desktop).await.unwrap();
        assert_eq!(delivery.command.command_id, targeted.command_id);
        broker.ack("desktop", delivery.delivery_id).await.unwrap();

        // a general-pool consumer picks up the remaining Any action.
        let server = ConsumerProfile::shared("server");
        let delivery = broker.receive_for(&server).await.unwrap();
        assert_eq!(delivery.command.command_id, any.command_id);
    }

    fn action_command() -> ActionCommand {
        ActionCommand {
            command_id: Uuid::new_v4(),
            workflow_run_id: Uuid::now_v7(),
            workflow_node_run_id: Uuid::now_v7(),
            node_id: "node-a".into(),
            action: WorkflowAction {
                provider: "test".into(),
                function: "execute".into(),
                timeout_seconds: 60,
                configuration: runinator_models::workflows::WorkflowObject::default(),
                mcp_enabled: false,
                tags: Vec::new(),
                required_labels: Default::default(),
            },
            attempt: 1,
            parameters: json!({}),
            target: Default::default(),
            trace_id: Uuid::nil(),
            trace_context: Default::default(),
        }
    }
}
