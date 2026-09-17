#[allow(unused_imports)]
use super::*;

#[derive(Clone)]
pub struct InMemoryBroker {
    pub(super) state: Arc<Mutex<BrokerState>>,
    pub(super) control_notify: Arc<Notify>,
    pub(super) agent_notify: Arc<Notify>,
    pub(super) effect_notify: Arc<Notify>,
    pub(super) effect_result_notify: Arc<Notify>,
    pub(super) wake_notify: Arc<Notify>,
    pub(super) ingress_notify: Arc<Notify>,
    // fan-out: every subscriber drains its own receiver of every published event.
    pub(super) event_tx: broadcast::Sender<EventDelivery>,
    pub(super) event_subscribers: Arc<Mutex<HashMap<String, EventReceiver>>>,
    pub(super) lease_duration: Duration,
}

impl InMemoryBroker {
    pub(super) const DEFAULT_LEASE_DURATION: Duration = Duration::from_secs(30);

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
            control_notify: Arc::new(Notify::new()),
            agent_notify: Arc::new(Notify::new()),
            effect_notify: Arc::new(Notify::new()),
            effect_result_notify: Arc::new(Notify::new()),
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
    pub(super) async fn receive_control_matching(
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

    pub(super) async fn receive_agent_matching(
        &self,
        matches: impl Fn(&AgentDelivery) -> bool,
    ) -> Result<AgentDelivery, BrokerError> {
        loop {
            let notified = self.agent_notify.notified();
            tokio::pin!(notified);
            notified.as_mut().enable();
            if let Some(delivery) = {
                let mut guard = self.state.lock();
                guard.reclaim_expired_agent(Instant::now());
                guard
                    .agent_queue
                    .retain(|delivery| delivery.command.expires_at > chrono::Utc::now());
                let index = guard.agent_queue.iter().position(&matches);
                match index.and_then(|index| guard.agent_queue.remove(index)) {
                    Some(delivery) => {
                        guard.agent_inflight.insert(
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
    pub(super) fn event_receiver(&self, consumer: &str) -> EventReceiver {
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
    fn supports_workflow_effect_channels(&self) -> bool {
        true
    }

    fn supports_agent_channel(&self) -> bool {
        true
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
        let Some(leased) = guard.control_inflight.remove(&delivery_id) else {
            return Err(BrokerError::UnknownDelivery(delivery_id));
        };

        guard
            .control_queue
            .push_front(redeliver_control(leased.delivery));
        drop(guard);
        self.control_notify.notify_waiters();
        Ok(())
    }

    async fn publish_agent(&self, command: AgentCommand) -> Result<(), BrokerError> {
        let mut guard = self.state.lock();
        guard.agent_queue.push_back(command.into());
        drop(guard);
        self.agent_notify.notify_waiters();
        Ok(())
    }

    async fn receive_agent(&self, _consumer: &str) -> Result<AgentDelivery, BrokerError> {
        self.receive_agent_matching(|_| true).await
    }

    async fn receive_agent_for(
        &self,
        profile: &ConsumerProfile,
    ) -> Result<AgentDelivery, BrokerError> {
        self.receive_agent_matching(|delivery| delivery.command.target.matches(profile))
            .await
    }

    async fn ack_agent(&self, _consumer: &str, delivery_id: Uuid) -> Result<(), BrokerError> {
        let mut guard = self.state.lock();
        if guard.agent_inflight.remove(&delivery_id).is_some() {
            Ok(())
        } else {
            Err(BrokerError::UnknownDelivery(delivery_id))
        }
    }

    async fn nack_agent(&self, _consumer: &str, delivery_id: Uuid) -> Result<(), BrokerError> {
        let mut guard = self.state.lock();
        let Some(leased) = guard.agent_inflight.remove(&delivery_id) else {
            return Err(BrokerError::UnknownDelivery(delivery_id));
        };

        guard
            .agent_queue
            .push_front(redeliver_agent(leased.delivery));
        drop(guard);
        self.agent_notify.notify_waiters();
        Ok(())
    }

    async fn publish_effect(&self, message: EffectMessage) -> Result<(), BrokerError> {
        if message.is_expired_at(chrono::Utc::now()) {
            return Ok(());
        }
        let mut guard = self.state.lock();
        let dedupe = message.dedupe_key_or_hash();
        if !guard.effect_dedupe.insert(dedupe.clone()) {
            return Err(BrokerError::Duplicate(dedupe));
        }
        guard.effect_queue.push_back(message.into());
        drop(guard);
        self.effect_notify.notify_one();
        Ok(())
    }

    async fn receive_effect(&self, _consumer: &str) -> Result<EffectDelivery, BrokerError> {
        loop {
            if let Some(delivery) = {
                let mut guard = self.state.lock();
                let now = chrono::Utc::now();
                guard.reclaim_expired_effects(Instant::now(), now);
                guard.drop_expired_effects(now);
                guard.effect_queue.pop_front().inspect(|delivery| {
                    guard.effect_inflight.insert(
                        delivery.delivery_id,
                        Leased {
                            delivery: delivery.clone(),
                            leased_until: Instant::now() + self.lease_duration,
                        },
                    );
                })
            } {
                return Ok(delivery);
            }
            tokio::select! { _ = self.effect_notify.notified() => {}, _ = tokio::time::sleep(self.lease_duration) => {} }
        }
    }

    async fn ack_effect(&self, _consumer: &str, delivery_id: Uuid) -> Result<(), BrokerError> {
        let mut guard = self.state.lock();
        if let Some(leased) = guard.effect_inflight.remove(&delivery_id) {
            guard.effect_dedupe.remove(&leased.delivery.dedupe_key);
            Ok(())
        } else {
            Err(BrokerError::UnknownDelivery(delivery_id))
        }
    }

    async fn nack_effect(&self, _consumer: &str, delivery_id: Uuid) -> Result<(), BrokerError> {
        let mut guard = self.state.lock();
        let Some(leased) = guard.effect_inflight.remove(&delivery_id) else {
            return Err(BrokerError::UnknownDelivery(delivery_id));
        };

        if leased.delivery.is_expired_at(chrono::Utc::now()) {
            guard.effect_dedupe.remove(&leased.delivery.dedupe_key);
            return Ok(());
        }
        guard
            .effect_queue
            .push_front(redeliver_effect(leased.delivery));
        drop(guard);
        self.effect_notify.notify_one();
        Ok(())
    }

    async fn publish_effect_result(&self, message: EffectResultMessage) -> Result<(), BrokerError> {
        let mut guard = self.state.lock();
        let dedupe = message.dedupe_key_or_hash();
        if !guard.effect_result_dedupe.insert(dedupe.clone()) {
            return Err(BrokerError::Duplicate(dedupe));
        }
        guard.effect_result_queue.push_back(message.into());
        drop(guard);
        self.effect_result_notify.notify_one();
        Ok(())
    }

    async fn receive_effect_result(
        &self,
        _consumer: &str,
    ) -> Result<EffectResultDelivery, BrokerError> {
        loop {
            if let Some(delivery) = {
                let mut guard = self.state.lock();
                guard.reclaim_expired_effect_results(Instant::now());
                guard.effect_result_queue.pop_front().inspect(|delivery| {
                    guard.effect_result_inflight.insert(
                        delivery.delivery_id,
                        Leased {
                            delivery: delivery.clone(),
                            leased_until: Instant::now() + self.lease_duration,
                        },
                    );
                })
            } {
                return Ok(delivery);
            }
            tokio::select! { _ = self.effect_result_notify.notified() => {}, _ = tokio::time::sleep(self.lease_duration) => {} }
        }
    }

    async fn ack_effect_result(
        &self,
        _consumer: &str,
        delivery_id: Uuid,
    ) -> Result<(), BrokerError> {
        let mut guard = self.state.lock();
        let Some(leased) = guard.effect_result_inflight.remove(&delivery_id) else {
            return Err(BrokerError::UnknownDelivery(delivery_id));
        };

        guard
            .effect_result_dedupe
            .remove(&leased.delivery.dedupe_key);
        Ok(())
    }

    async fn nack_effect_result(
        &self,
        _consumer: &str,
        delivery_id: Uuid,
    ) -> Result<(), BrokerError> {
        let mut guard = self.state.lock();
        let Some(leased) = guard.effect_result_inflight.remove(&delivery_id) else {
            return Err(BrokerError::UnknownDelivery(delivery_id));
        };

        guard
            .effect_result_queue
            .push_front(redeliver_effect_result(leased.delivery));
        drop(guard);
        self.effect_result_notify.notify_one();
        Ok(())
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
        let Some(leased) = guard.wake_inflight.remove(&delivery_id) else {
            return Err(BrokerError::UnknownDelivery(delivery_id));
        };

        guard.wake_queue.push_front(redeliver_wake(leased.delivery));
        drop(guard);
        self.wake_notify.notify_one();
        Ok(())
    }

    async fn publish_ingress(&self, message: IngressMessage) -> Result<(), BrokerError> {
        let mut guard = self.state.lock();
        let dedupe = message.dedupe_key_or_hash();
        if !guard.ingress_dedupe.insert(dedupe.clone()) {
            if matches!(
                message.command,
                runinator_comm::WsIngressCommand::ReplicaAvailability { .. }
            ) {
                return Ok(());
            }
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
        let Some(leased) = guard.ingress_inflight.remove(&delivery_id) else {
            return Err(BrokerError::UnknownDelivery(delivery_id));
        };

        guard
            .ingress_queue
            .push_front(redeliver_ingress(leased.delivery));
        drop(guard);
        self.ingress_notify.notify_one();
        Ok(())
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
