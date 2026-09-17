use crate::{
    AgentCommand, AgentDelivery, Broker, BrokerError, ConsumerProfile, ControlCommand,
    ControlDelivery, EffectDelivery, EffectMessage, EffectResultDelivery, EffectResultMessage,
    EventDelivery, EventMessage, IngressDelivery, IngressMessage, WakeDelivery, WakeMessage,
};
use async_trait::async_trait;
use parking_lot::Mutex;
use std::collections::{HashMap, HashSet, VecDeque};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::{broadcast, Mutex as AsyncMutex, Notify};
use uuid::Uuid;

const EVENT_CHANNEL_CAPACITY: usize = 1024;

type EventReceiver = Arc<AsyncMutex<broadcast::Receiver<EventDelivery>>>;

fn expired_ids<T>(inflight: &HashMap<Uuid, Leased<T>>, now: Instant) -> Vec<Uuid> {
    inflight
        .iter()
        .filter_map(|(id, leased)| (leased.leased_until <= now).then_some(*id))
        .collect()
}

fn redeliver_control(delivery: ControlDelivery) -> ControlDelivery {
    ControlDelivery {
        delivery_id: Uuid::new_v4(),
        ..delivery
    }
}

fn redeliver_agent(delivery: AgentDelivery) -> AgentDelivery {
    AgentDelivery {
        delivery_id: Uuid::new_v4(),
        ..delivery
    }
}

fn redeliver_effect(delivery: EffectDelivery) -> EffectDelivery {
    EffectDelivery {
        delivery_id: Uuid::new_v4(),
        ..delivery
    }
}

fn redeliver_effect_result(delivery: EffectResultDelivery) -> EffectResultDelivery {
    EffectResultDelivery {
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

mod broker_state;
use broker_state::BrokerState;

mod leased;
use leased::Leased;

mod in_memory_broker;
pub use in_memory_broker::InMemoryBroker;
