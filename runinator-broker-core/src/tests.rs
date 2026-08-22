//! backend-agnostic tests for the `Broker` trait's default-provided behavior (methods every backend
//! gets for free unless it overrides them), independent of any concrete backend.

use std::sync::Mutex;

use async_trait::async_trait;
use runinator_comm::{ActionTarget, ConsumerProfile, EffectCommand, EffectExecutor};
use runinator_models::workflow_vm::WorkflowEffectRequest;
use uuid::Uuid;

use crate::{
    Broker, BrokerError, ControlCommand, ControlDelivery, EffectDelivery, EffectMessage,
    EventDelivery, EventMessage, IngressDelivery, IngressMessage, WakeDelivery, WakeMessage,
};

/// a fake `Broker` exercising only the default `receive_effect_for`: `receive_effect` pops a fixed,
/// pre-seeded queue of deliveries in order; `nack_effect` just records the delivery id it was called
/// with. every other method is unreachable in this test and panics if called.
struct FakeBroker {
    queue: Mutex<Vec<EffectDelivery>>,
    nacked: Mutex<Vec<Uuid>>,
}

impl FakeBroker {
    fn with_deliveries(deliveries: Vec<EffectDelivery>) -> Self {
        // `receive_effect` pops from the front, so reverse once up front and `pop()` (removes the
        // back).
        let mut queue = deliveries;
        queue.reverse();
        Self {
            queue: Mutex::new(queue),
            nacked: Mutex::new(Vec::new()),
        }
    }
}

#[async_trait]
impl Broker for FakeBroker {
    async fn receive_effect(&self, _consumer: &str) -> Result<EffectDelivery, BrokerError> {
        self.queue
            .lock()
            .unwrap()
            .pop()
            .ok_or_else(|| BrokerError::Internal("queue exhausted".into()))
    }

    async fn ack_effect(&self, _consumer: &str, _delivery_id: Uuid) -> Result<(), BrokerError> {
        Ok(())
    }

    async fn nack_effect(&self, _consumer: &str, delivery_id: Uuid) -> Result<(), BrokerError> {
        self.nacked.lock().unwrap().push(delivery_id);
        Ok(())
    }

    async fn publish_control(&self, _command: ControlCommand) -> Result<(), BrokerError> {
        unimplemented!("not exercised by this test")
    }

    async fn receive_control(&self, _consumer: &str) -> Result<ControlDelivery, BrokerError> {
        unimplemented!("not exercised by this test")
    }

    async fn ack_control(&self, _consumer: &str, _delivery_id: Uuid) -> Result<(), BrokerError> {
        unimplemented!("not exercised by this test")
    }

    async fn nack_control(&self, _consumer: &str, _delivery_id: Uuid) -> Result<(), BrokerError> {
        unimplemented!("not exercised by this test")
    }

    async fn publish_wake(&self, _message: WakeMessage) -> Result<(), BrokerError> {
        unimplemented!("not exercised by this test")
    }

    async fn receive_wake(&self, _consumer: &str) -> Result<WakeDelivery, BrokerError> {
        unimplemented!("not exercised by this test")
    }

    async fn ack_wake(&self, _consumer: &str, _delivery_id: Uuid) -> Result<(), BrokerError> {
        unimplemented!("not exercised by this test")
    }

    async fn nack_wake(&self, _consumer: &str, _delivery_id: Uuid) -> Result<(), BrokerError> {
        unimplemented!("not exercised by this test")
    }

    async fn publish_ingress(&self, _message: IngressMessage) -> Result<(), BrokerError> {
        unimplemented!("not exercised by this test")
    }

    async fn receive_ingress(&self, _consumer: &str) -> Result<IngressDelivery, BrokerError> {
        unimplemented!("not exercised by this test")
    }

    async fn ack_ingress(&self, _consumer: &str, _delivery_id: Uuid) -> Result<(), BrokerError> {
        unimplemented!("not exercised by this test")
    }

    async fn nack_ingress(&self, _consumer: &str, _delivery_id: Uuid) -> Result<(), BrokerError> {
        unimplemented!("not exercised by this test")
    }

    async fn publish_event(&self, _message: EventMessage) -> Result<(), BrokerError> {
        unimplemented!("not exercised by this test")
    }

    async fn receive_event(&self, _consumer: &str) -> Result<EventDelivery, BrokerError> {
        unimplemented!("not exercised by this test")
    }
}

fn delivery(executor: EffectExecutor, target: ActionTarget) -> EffectDelivery {
    let command = EffectCommand {
        version: 1,
        command_id: Uuid::new_v4(),
        effect_id: Uuid::now_v7(),
        workflow_run_id: Uuid::now_v7(),
        continuation_id: Uuid::now_v7(),
        attempt: 1,
        request: WorkflowEffectRequest::Timer { due_at: 42 },
        executor,
        target,
        trace_id: Uuid::nil(),
        trace_context: Default::default(),
        idempotency_key: Uuid::new_v4().to_string(),
        notification_delivery_id: None,
    };
    EffectDelivery::from(EffectMessage {
        command,
        dedupe_key: None,
        enqueued_at: chrono::Utc::now(),
    })
}

#[tokio::test]
async fn default_receive_effect_for_requeues_mismatches_and_returns_the_first_match() {
    let mismatched_one = delivery(
        EffectExecutor::Provider,
        ActionTarget::Labels {
            selector: [("runner".to_string(), "other".to_string())].into(),
        },
    );
    // an infrastructure effect must never be claimed by a provider worker, whatever its target.
    let mismatched_two = delivery(EffectExecutor::Infrastructure, ActionTarget::Any);
    let matching = delivery(
        EffectExecutor::Provider,
        ActionTarget::Labels {
            selector: [("runner".to_string(), "creds-sync".to_string())].into(),
        },
    );
    let matching_id = matching.command.command_id;
    let mismatched_ids = [mismatched_one.delivery_id, mismatched_two.delivery_id];

    let broker = FakeBroker::with_deliveries(vec![
        mismatched_one,
        mismatched_two,
        matching,
        // never reached: proves the loop stops at the first match instead of draining everything.
        delivery(EffectExecutor::Provider, ActionTarget::Any),
    ]);

    let profile = ConsumerProfile::shared("desktop")
        .with_labels([("runner".to_string(), "creds-sync".to_string())].into())
        .exclusive();

    let result = broker.receive_effect_for(&profile).await.unwrap();
    assert_eq!(result.command.command_id, matching_id);

    let nacked = broker.nacked.lock().unwrap().clone();
    assert_eq!(nacked.len(), 2);
    assert!(mismatched_ids.iter().all(|id| nacked.contains(id)));
    // one delivery (the Any one) must still be sitting in the queue, untouched.
    assert_eq!(broker.queue.lock().unwrap().len(), 1);
}
