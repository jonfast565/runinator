use crate::{
    Broker, BrokerDelivery, BrokerError, BrokerMessage, ControlCommand, ControlDelivery,
    IngressDelivery, IngressMessage, ResultDelivery, ResultMessage, WakeDelivery, WakeMessage,
};
use async_trait::async_trait;
use parking_lot::Mutex;
use std::collections::{HashMap, HashSet, VecDeque};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::Notify;
use uuid::Uuid;

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

#[derive(Clone)]
pub struct InMemoryBroker {
    state: Arc<Mutex<BrokerState>>,
    notify: Arc<Notify>,
    control_notify: Arc<Notify>,
    result_notify: Arc<Notify>,
    wake_notify: Arc<Notify>,
    ingress_notify: Arc<Notify>,
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
        Self {
            state: Arc::new(Mutex::new(BrokerState::default())),
            notify: Arc::new(Notify::new()),
            control_notify: Arc::new(Notify::new()),
            result_notify: Arc::new(Notify::new()),
            wake_notify: Arc::new(Notify::new()),
            ingress_notify: Arc::new(Notify::new()),
            lease_duration: Self::DEFAULT_LEASE_DURATION,
        }
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
        self.notify.notify_one();
        Ok(())
    }

    async fn receive(&self, _consumer: &str) -> Result<BrokerDelivery, BrokerError> {
        loop {
            if let Some(delivery) = {
                let mut guard = self.state.lock();
                reclaim_expired_actions(&mut guard, Instant::now());
                if let Some(delivery) = guard.queue.pop_front() {
                    guard.inflight.insert(
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
                _ = self.notify.notified() => {}
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
            Ok(())
        } else {
            Err(BrokerError::UnknownDelivery(delivery_id))
        }
    }

    async fn publish_control(&self, command: ControlCommand) -> Result<(), BrokerError> {
        let mut guard = self.state.lock();
        guard.control_queue.push_back(command.into());
        drop(guard);
        self.control_notify.notify_one();
        Ok(())
    }

    async fn receive_control(&self, _consumer: &str) -> Result<ControlDelivery, BrokerError> {
        loop {
            if let Some(delivery) = {
                let mut guard = self.state.lock();
                reclaim_expired_control(&mut guard, Instant::now());
                if let Some(delivery) = guard.control_queue.pop_front() {
                    guard.control_inflight.insert(
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
                _ = self.control_notify.notified() => {}
                _ = tokio::time::sleep(self.lease_duration) => {}
            }
        }
    }

    async fn ack_control(&self, _consumer: &str, delivery_id: Uuid) -> Result<(), BrokerError> {
        let mut guard = self.state.lock();
        if guard.control_inflight.remove(&delivery_id).is_some() {
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
            Ok(())
        } else {
            Err(BrokerError::UnknownDelivery(delivery_id))
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
        let command =
            runinator_comm::WakeCommand::new(7, 42, "node-a".into(), Utc::now(), Uuid::new_v4());
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
        let command = runinator_comm::WsIngressCommand::drive(7, 42, "node-a".into());
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
            runinator_comm::WsIngressCommand::Drive {
                ready_node_id: 7,
                ..
            }
        ));
        broker
            .ack_ingress("ws", delivery.delivery_id)
            .await
            .unwrap();
    }

    fn action_command() -> ActionCommand {
        ActionCommand {
            command_id: Uuid::new_v4(),
            workflow_run_id: 42,
            workflow_node_run_id: 99,
            node_id: "node-a".into(),
            action: WorkflowAction {
                provider: "test".into(),
                function: "execute".into(),
                timeout_seconds: 60,
                configuration: runinator_models::workflows::WorkflowObject::default(),
                mcp_enabled: false,
                tags: Vec::new(),
            },
            attempt: 1,
            parameters: json!({}),
        }
    }
}
