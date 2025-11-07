use crate::{Broker, BrokerDelivery, BrokerError, BrokerMessage};
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
}

#[derive(Clone, Default)]
pub struct InMemoryBroker {
    state: Arc<Mutex<BrokerState>>,
    notify: Arc<Notify>,
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

    async fn poll(&self, _consumer: &str) -> Result<Option<BrokerDelivery>, BrokerError> {
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
                return Ok(Some(delivery));
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
}
