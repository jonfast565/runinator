use crate::{
    Broker, BrokerDelivery, BrokerError, BrokerMessage, ControlCommand, ControlDelivery,
    ResultDelivery, ResultMessage,
};
use async_trait::async_trait;
use parking_lot::Mutex;
use std::collections::{HashMap, HashSet, VecDeque};
use std::sync::Arc;
use tokio::sync::Notify;
use uuid::Uuid;

#[derive(Default)]
struct BrokerState {
    queue: VecDeque<BrokerDelivery>,
    inflight: HashMap<Uuid, BrokerDelivery>,
    dedupe: HashSet<String>,
    control_queue: VecDeque<ControlDelivery>,
    control_inflight: HashMap<Uuid, ControlDelivery>,
    result_queue: VecDeque<ResultDelivery>,
    result_inflight: HashMap<Uuid, ResultDelivery>,
    result_dedupe: HashSet<String>,
}

#[derive(Clone, Default)]
pub struct InMemoryBroker {
    state: Arc<Mutex<BrokerState>>,
    notify: Arc<Notify>,
    control_notify: Arc<Notify>,
    result_notify: Arc<Notify>,
}

impl InMemoryBroker {
    pub fn new() -> Self {
        Self::default()
    }
}

#[async_trait]
impl Broker for InMemoryBroker {
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
                if let Some(delivery) = guard.queue.pop_front() {
                    guard
                        .inflight
                        .insert(delivery.delivery_id, delivery.clone());
                    Some(delivery)
                } else {
                    None
                }
            } {
                return Ok(delivery);
            }

            self.notify.notified().await;
        }
    }

    async fn ack(&self, _consumer: &str, delivery_id: Uuid) -> Result<(), BrokerError> {
        let mut guard = self.state.lock();
        if let Some(delivery) = guard.inflight.remove(&delivery_id) {
            guard.dedupe.remove(&delivery.dedupe_key);
            Ok(())
        } else {
            Err(BrokerError::UnknownDelivery(delivery_id))
        }
    }

    async fn nack(&self, _consumer: &str, delivery_id: Uuid) -> Result<(), BrokerError> {
        let mut guard = self.state.lock();
        if let Some(delivery) = guard.inflight.remove(&delivery_id) {
            guard.queue.push_front(delivery);
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
                if let Some(delivery) = guard.control_queue.pop_front() {
                    guard
                        .control_inflight
                        .insert(delivery.delivery_id, delivery.clone());
                    Some(delivery)
                } else {
                    None
                }
            } {
                return Ok(delivery);
            }

            self.control_notify.notified().await;
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
                if let Some(delivery) = guard.result_queue.pop_front() {
                    guard
                        .result_inflight
                        .insert(delivery.delivery_id, delivery.clone());
                    Some(delivery)
                } else {
                    None
                }
            } {
                return Ok(delivery);
            }

            self.result_notify.notified().await;
        }
    }

    async fn ack_result(&self, _consumer: &str, delivery_id: Uuid) -> Result<(), BrokerError> {
        let mut guard = self.state.lock();
        if let Some(delivery) = guard.result_inflight.remove(&delivery_id) {
            guard.result_dedupe.remove(&delivery.dedupe_key);
            Ok(())
        } else {
            Err(BrokerError::UnknownDelivery(delivery_id))
        }
    }

    async fn nack_result(&self, _consumer: &str, delivery_id: Uuid) -> Result<(), BrokerError> {
        let mut guard = self.state.lock();
        if let Some(delivery) = guard.result_inflight.remove(&delivery_id) {
            guard.result_queue.push_front(delivery);
            Ok(())
        } else {
            Err(BrokerError::UnknownDelivery(delivery_id))
        }
    }
}
