#![cfg(feature = "kafka")]

use chrono::Utc;
use runinator_broker::{
    adapters::kafka::{KafkaBroker, KafkaBrokerConfig},
    Broker, BrokerMessage, ControlCommand, ResultMessage,
};
use runinator_comm::{ActionCommand, ControlKind, WorkflowResultEvent};
use runinator_models::json;
use runinator_models::{runs::NewRunChunk, workflows::WorkflowAction};
use tokio::time::{timeout, Duration};
use uuid::Uuid;

fn kafka_broker() -> Option<KafkaBroker> {
    let bootstrap = match std::env::var("RUNINATOR_KAFKA_BOOTSTRAP") {
        Ok(bootstrap) => bootstrap,
        Err(_) => {
            eprintln!("skipping kafka integration test; set RUNINATOR_KAFKA_BOOTSTRAP");
            return None;
        }
    };
    let action_topic = std::env::var("RUNINATOR_KAFKA_ACTION_TOPIC")
        .unwrap_or_else(|_| "runinator.actions".into());
    let control_topic = std::env::var("RUNINATOR_KAFKA_CONTROL_TOPIC")
        .unwrap_or_else(|_| "runinator.control".into());
    let result_topic = std::env::var("RUNINATOR_KAFKA_RESULT_TOPIC")
        .unwrap_or_else(|_| "runinator.results".into());

    Some(
        KafkaBroker::new(
            KafkaBrokerConfig::new(bootstrap)
                .with_topics(action_topic, control_topic, result_topic)
                .with_client_id(format!("runinator-test-{}", Uuid::new_v4())),
        )
        .unwrap(),
    )
}

#[tokio::test]
#[ignore = "requires a reachable Kafka broker and pre-created topics"]
async fn kafka_broker_delivers_published_messages() {
    let Some(broker) = kafka_broker() else {
        return;
    };
    let command = action_command();
    let command_id = command.command_id;
    broker
        .publish(BrokerMessage {
            command,
            dedupe_key: Some(command_id.to_string()),
            enqueued_at: Utc::now(),
        })
        .await
        .unwrap();

    let consumer = format!("test-actions-{}", Uuid::new_v4());
    loop {
        let delivery = timeout(Duration::from_secs(10), broker.receive(&consumer))
            .await
            .unwrap()
            .unwrap();
        broker.ack(&consumer, delivery.delivery_id).await.unwrap();
        if delivery.command.command_id == command_id {
            assert_eq!(delivery.command.workflow_run_id, Uuid::from_u128(42));
            break;
        }
    }
}

#[tokio::test]
#[ignore = "requires a reachable Kafka broker and pre-created topics"]
async fn kafka_broker_delivers_control_messages() {
    let Some(broker) = kafka_broker() else {
        return;
    };
    broker
        .publish_control(ControlCommand::new(
            Uuid::from_u128(4242),
            ControlKind::Cancel,
        ))
        .await
        .unwrap();

    let consumer = format!("test-control-{}", Uuid::new_v4());
    loop {
        let delivery = timeout(Duration::from_secs(10), broker.receive_control(&consumer))
            .await
            .unwrap()
            .unwrap();
        broker
            .ack_control(&consumer, delivery.delivery_id)
            .await
            .unwrap();
        if delivery.command.workflow_run_id == 4242 {
            assert!(matches!(delivery.command.kind, ControlKind::Cancel));
            break;
        }
    }
}

#[tokio::test]
#[ignore = "requires a reachable Kafka broker and pre-created topics"]
async fn kafka_broker_delivers_result_events() {
    let Some(broker) = kafka_broker() else {
        return;
    };
    let command = action_command();
    let event = WorkflowResultEvent::chunk(
        &command,
        NewRunChunk {
            stream: "log".into(),
            content: "hello".into(),
        },
    );
    let event_id = event.event_id;
    broker
        .publish_result(ResultMessage {
            event,
            dedupe_key: Some(event_id.to_string()),
            enqueued_at: Utc::now(),
        })
        .await
        .unwrap();

    let consumer = format!("test-results-{}", Uuid::new_v4());
    loop {
        let delivery = timeout(Duration::from_secs(10), broker.receive_result(&consumer))
            .await
            .unwrap()
            .unwrap();
        broker
            .ack_result(&consumer, delivery.delivery_id)
            .await
            .unwrap();
        if delivery.event.event_id == event_id {
            assert_eq!(delivery.event.workflow_node_run_id, Uuid::from_u128(99));
            break;
        }
    }
}

#[tokio::test]
#[ignore = "requires a reachable Kafka broker and pre-created topics"]
async fn kafka_broker_nack_redelivers_messages() {
    let Some(broker) = kafka_broker() else {
        return;
    };
    let command = action_command();
    let command_id = command.command_id;
    let consumer = format!("test-nack-{}", Uuid::new_v4());
    broker
        .publish(BrokerMessage {
            command,
            dedupe_key: Some(command_id.to_string()),
            enqueued_at: Utc::now(),
        })
        .await
        .unwrap();

    let delivery = loop {
        let delivery = timeout(Duration::from_secs(10), broker.receive(&consumer))
            .await
            .unwrap()
            .unwrap();
        if delivery.command.command_id == command_id {
            break delivery;
        }
        broker.ack(&consumer, delivery.delivery_id).await.unwrap();
    };
    broker.nack(&consumer, delivery.delivery_id).await.unwrap();

    let redelivery = timeout(Duration::from_secs(10), broker.receive(&consumer))
        .await
        .unwrap()
        .unwrap();
    assert_eq!(redelivery.command.command_id, command_id);
    broker.ack(&consumer, redelivery.delivery_id).await.unwrap();
}

fn action_command() -> ActionCommand {
    ActionCommand {
        command_id: Uuid::new_v4(),
        workflow_run_id: Uuid::from_u128(42),
        workflow_node_run_id: Uuid::from_u128(99),
        node_id: "run".into(),
        action: WorkflowAction {
            provider: "test".into(),
            function: "execute".into(),
            timeout_seconds: 60,
            configuration: runinator_models::workflows::WorkflowObject::default(),
            mcp_enabled: false,
            tags: Vec::new(),
        },
        attempt: 1,
        parameters: json!({ "value": true }),
        trace_id: Uuid::nil(),
    }
}
