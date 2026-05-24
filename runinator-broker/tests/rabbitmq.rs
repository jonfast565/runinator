#![cfg(feature = "rabbitmq")]

use chrono::Utc;
use runinator_broker::{
    adapters::rabbitmq::{RabbitMqBroker, RabbitMqBrokerConfig},
    Broker, BrokerMessage, ControlCommand, ResultMessage,
};
use runinator_comm::{ActionCommand, ControlKind, WorkflowResultEvent};
use runinator_models::{runs::NewRunChunk, workflows::WorkflowAction};
use serde_json::json;
use tokio::time::{timeout, Duration};
use uuid::Uuid;

async fn rabbitmq_broker() -> Option<RabbitMqBroker> {
    let uri = match std::env::var("RUNINATOR_RABBITMQ_URI") {
        Ok(uri) => uri,
        Err(_) => {
            eprintln!("skipping rabbitmq integration test; set RUNINATOR_RABBITMQ_URI");
            return None;
        }
    };
    let action_queue = std::env::var("RUNINATOR_RABBITMQ_ACTION_QUEUE")
        .unwrap_or_else(|_| format!("runinator.test.actions.{}", Uuid::new_v4()));
    let control_queue = std::env::var("RUNINATOR_RABBITMQ_CONTROL_QUEUE")
        .unwrap_or_else(|_| format!("runinator.test.control.{}", Uuid::new_v4()));
    let result_queue = std::env::var("RUNINATOR_RABBITMQ_RESULT_QUEUE")
        .unwrap_or_else(|_| format!("runinator.test.results.{}", Uuid::new_v4()));

    Some(
        RabbitMqBroker::connect(
            RabbitMqBrokerConfig::new(uri)
                .with_queues(action_queue, control_queue, result_queue)
                .with_client_id(format!("runinator-test-{}", Uuid::new_v4())),
        )
        .await
        .unwrap(),
    )
}

#[tokio::test]
#[ignore = "requires a reachable RabbitMQ broker"]
async fn rabbitmq_broker_delivers_published_messages() {
    let Some(broker) = rabbitmq_broker().await else {
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
    let delivery = timeout(Duration::from_secs(10), broker.receive(&consumer))
        .await
        .unwrap()
        .unwrap();
    assert_eq!(delivery.command.command_id, command_id);
    assert_eq!(delivery.command.workflow_run_id, 42);
    broker.ack(&consumer, delivery.delivery_id).await.unwrap();
}

#[tokio::test]
#[ignore = "requires a reachable RabbitMQ broker"]
async fn rabbitmq_broker_delivers_control_messages() {
    let Some(broker) = rabbitmq_broker().await else {
        return;
    };
    broker
        .publish_control(ControlCommand::new(4242, ControlKind::Cancel))
        .await
        .unwrap();

    let consumer = format!("test-control-{}", Uuid::new_v4());
    let delivery = timeout(Duration::from_secs(10), broker.receive_control(&consumer))
        .await
        .unwrap()
        .unwrap();
    assert_eq!(delivery.command.workflow_run_id, 4242);
    assert!(matches!(delivery.command.kind, ControlKind::Cancel));
    broker
        .ack_control(&consumer, delivery.delivery_id)
        .await
        .unwrap();
}

#[tokio::test]
#[ignore = "requires a reachable RabbitMQ broker"]
async fn rabbitmq_broker_delivers_result_events() {
    let Some(broker) = rabbitmq_broker().await else {
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
    let delivery = timeout(Duration::from_secs(10), broker.receive_result(&consumer))
        .await
        .unwrap()
        .unwrap();
    assert_eq!(delivery.event.event_id, event_id);
    assert_eq!(delivery.event.workflow_node_run_id, 99);
    broker
        .ack_result(&consumer, delivery.delivery_id)
        .await
        .unwrap();
}

#[tokio::test]
#[ignore = "requires a reachable RabbitMQ broker"]
async fn rabbitmq_broker_nack_redelivers_messages() {
    let Some(broker) = rabbitmq_broker().await else {
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

    let delivery = timeout(Duration::from_secs(10), broker.receive(&consumer))
        .await
        .unwrap()
        .unwrap();
    assert_eq!(delivery.command.command_id, command_id);
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
        workflow_run_id: 42,
        workflow_node_run_id: 99,
        node_id: "run".into(),
        action: WorkflowAction {
            provider: "test".into(),
            function: "execute".into(),
            timeout_seconds: 60,
            configuration: json!({}),
            mcp_enabled: false,
            tags: Vec::new(),
        },
        attempt: 1,
        parameters: json!({ "value": true }),
    }
}
