//! backend-agnostic tests for the `Broker` trait's default-provided behavior (methods every backend
//! gets for free unless it overrides them), independent of any concrete backend.

use std::sync::Mutex;

use async_trait::async_trait;
use runinator_comm::{ActionCommand, ActionTarget, ConsumerProfile};
use runinator_models::{json, workflows::WorkflowAction};
use uuid::Uuid;

use crate::{
    Broker, BrokerDelivery, BrokerError, BrokerMessage, ControlCommand, ControlDelivery,
    EventDelivery, EventMessage, IngressDelivery, IngressMessage, ResultDelivery, ResultMessage,
    WakeDelivery, WakeMessage,
};

/// a fake `Broker` exercising only the default `receive_for`: `receive` pops a fixed, pre-seeded
/// queue of deliveries in order; `nack` just records the delivery id it was called with. every other
/// method is unreachable in this test and panics if called.
struct FakeBroker {
    queue: Mutex<Vec<BrokerDelivery>>,
    nacked: Mutex<Vec<Uuid>>,
}

impl FakeBroker {
    fn with_deliveries(deliveries: Vec<BrokerDelivery>) -> Self {
        // `receive` pops from the front, so reverse once up front and `pop()` (removes the back).
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
    async fn publish(&self, _message: BrokerMessage) -> Result<(), BrokerError> {
        unimplemented!("not exercised by this test")
    }

    async fn receive(&self, _consumer: &str) -> Result<BrokerDelivery, BrokerError> {
        self.queue
            .lock()
            .unwrap()
            .pop()
            .ok_or_else(|| BrokerError::Internal("queue exhausted".into()))
    }

    async fn ack(&self, _consumer: &str, _delivery_id: Uuid) -> Result<(), BrokerError> {
        Ok(())
    }

    async fn nack(&self, _consumer: &str, delivery_id: Uuid) -> Result<(), BrokerError> {
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

    async fn publish_result(&self, _message: ResultMessage) -> Result<(), BrokerError> {
        unimplemented!("not exercised by this test")
    }

    async fn receive_result(&self, _consumer: &str) -> Result<ResultDelivery, BrokerError> {
        unimplemented!("not exercised by this test")
    }

    async fn ack_result(&self, _consumer: &str, _delivery_id: Uuid) -> Result<(), BrokerError> {
        unimplemented!("not exercised by this test")
    }

    async fn nack_result(&self, _consumer: &str, _delivery_id: Uuid) -> Result<(), BrokerError> {
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

fn delivery(target: ActionTarget) -> BrokerDelivery {
    let command = ActionCommand {
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
        target,
        trace_id: Uuid::nil(),
        trace_context: Default::default(),
    };
    BrokerDelivery::from(BrokerMessage {
        command,
        dedupe_key: None,
        enqueued_at: chrono::Utc::now(),
    })
}

#[tokio::test]
async fn default_receive_for_requeues_mismatches_and_returns_the_first_match() {
    let mismatched_one = delivery(ActionTarget::Labels {
        selector: [("runner".to_string(), "other".to_string())].into(),
    });
    let mismatched_two = delivery(ActionTarget::Replica {
        replica_id: Uuid::now_v7(),
    });
    let matching = delivery(ActionTarget::Labels {
        selector: [("runner".to_string(), "creds-sync".to_string())].into(),
    });
    let matching_id = matching.command.command_id;
    let mismatched_ids = [mismatched_one.delivery_id, mismatched_two.delivery_id];

    let broker = FakeBroker::with_deliveries(vec![
        mismatched_one,
        mismatched_two,
        matching,
        // never reached: proves the loop stops at the first match instead of draining everything.
        delivery(ActionTarget::Any),
    ]);

    let profile = ConsumerProfile::shared("desktop")
        .with_labels([("runner".to_string(), "creds-sync".to_string())].into())
        .exclusive();

    let result = broker.receive_for(&profile).await.unwrap();
    assert_eq!(result.command.command_id, matching_id);

    let nacked = broker.nacked.lock().unwrap().clone();
    assert_eq!(nacked.len(), 2);
    assert!(mismatched_ids.iter().all(|id| nacked.contains(id)));
    // one delivery (the Any one) must still be sitting in the queue, untouched.
    assert_eq!(broker.queue.lock().unwrap().len(), 1);
}
