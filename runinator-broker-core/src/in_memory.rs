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
                guard.reclaim_expired_control(Instant::now());
                guard.drop_stale_control(chrono::Utc::now());
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
                guard.reclaim_expired_actions(Instant::now());
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
                guard.reclaim_expired_results(Instant::now());
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
                guard.reclaim_expired_wakes(Instant::now());
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
                guard.reclaim_expired_ingress(Instant::now());
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

impl BrokerState {
    fn reclaim_expired_actions(&mut self, now: Instant) {
        let expired = expired_ids(&self.inflight, now);
        for id in expired {
            if let Some(leased) = self.inflight.remove(&id) {
                self.queue.push_front(redeliver_action(leased.delivery));
            }
        }
    }

    fn reclaim_expired_control(&mut self, now: Instant) {
        let expired = expired_ids(&self.control_inflight, now);
        for id in expired {
            if let Some(leased) = self.control_inflight.remove(&id) {
                self.control_queue
                    .push_front(redeliver_control(leased.delivery));
            }
        }
    }

    /// drop queued controls that have gone stale: a control targeted at a replica that never
    /// returns has no consumer that can ever match it, and controls are immediate signals, so
    /// retaining one past the ttl only grows the queue (this broker also backs the long-lived
    /// http/tcp servers).
    fn drop_stale_control(&mut self, now: chrono::DateTime<chrono::Utc>) {
        self.control_queue.retain(|delivery| {
            (now - delivery.enqueued_at).num_seconds() < crate::STALE_CONTROL_TTL_SECONDS
        });
    }

    fn reclaim_expired_results(&mut self, now: Instant) {
        let expired = expired_ids(&self.result_inflight, now);
        for id in expired {
            if let Some(leased) = self.result_inflight.remove(&id) {
                self.result_queue
                    .push_front(redeliver_result(leased.delivery));
            }
        }
    }

    fn reclaim_expired_wakes(&mut self, now: Instant) {
        let expired = expired_ids(&self.wake_inflight, now);
        for id in expired {
            if let Some(leased) = self.wake_inflight.remove(&id) {
                self.wake_queue.push_front(redeliver_wake(leased.delivery));
            }
        }
    }

    fn reclaim_expired_ingress(&mut self, now: Instant) {
        let expired = expired_ids(&self.ingress_inflight, now);
        for id in expired {
            if let Some(leased) = self.ingress_inflight.remove(&id) {
                self.ingress_queue
                    .push_front(redeliver_ingress(leased.delivery));
            }
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
#[path = "in_memory_tests.rs"]
mod tests;
