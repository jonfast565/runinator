use runinator_broker::{Broker, in_memory::InMemoryBroker};
use runinator_comm::{ActionCommand, WorkflowResultEventKind};
use runinator_models::json;
use runinator_models::workflows::{WorkflowAction, WorkflowStatus};
use runinator_models::{
    providers::{ActionMetadata, ResultMetadata, RuninatorType},
    runs::{RunStatus, TaskExecutionResult},
};
use uuid::Uuid;

use crate::{build_broker, config::Config, default_provider_factory, output_sink::RunOutputSink};

#[tokio::test]
async fn build_broker_rejects_kafka_without_result_topic() {
    let mut config = test_config();
    config.broker_backend = "kafka".into();
    config.broker_result_topic = " ".into();

    let err = match build_broker(&config).await {
        Ok(_) => panic!("expected kafka result channel startup guard to fail"),
        Err(err) => err,
    };

    assert!(err.to_string().contains("Broker backend 'kafka'"));
    assert!(err.to_string().contains("non-empty workflow result topic"));
}

#[tokio::test]
async fn build_broker_rejects_rabbitmq_without_result_queue() {
    let mut config = test_config();
    config.broker_backend = "rabbitmq".into();
    config.broker_result_topic = "".into();

    let err = match build_broker(&config).await {
        Ok(_) => panic!("expected rabbitmq result channel startup guard to fail"),
        Err(err) => err,
    };

    assert!(err.to_string().contains("Broker backend 'rabbitmq'"));
    assert!(err.to_string().contains("non-empty workflow result queue"));
}

#[tokio::test]
async fn output_sink_publishes_result_events_to_broker() {
    let broker = std::sync::Arc::new(InMemoryBroker::new());
    let command = action_command();
    let sink = RunOutputSink::new(
        command.clone(),
        broker.clone(),
        tokio::runtime::Handle::current(),
    );

    sink.emit_log("hello".into());
    sink.flush().await.unwrap();
    sink.publish_status(
        WorkflowStatus::Succeeded,
        Some(json!({ "success": true })),
        Some("done".into()),
    )
    .await
    .unwrap();

    let chunk_delivery = broker.receive_result("test-ws").await.unwrap();
    assert_eq!(chunk_delivery.event.command_id, command.command_id);
    match chunk_delivery.event.kind {
        WorkflowResultEventKind::Chunk { chunk } => {
            assert_eq!(chunk.stream, "log");
            assert_eq!(chunk.content, "hello");
        }
        _ => panic!("expected chunk result event"),
    }

    let status_delivery = broker.receive_result("test-ws").await.unwrap();
    match status_delivery.event.kind {
        WorkflowResultEventKind::Status {
            status, message, ..
        } => {
            assert_eq!(status, WorkflowStatus::Succeeded);
            assert_eq!(message.as_deref(), Some("done"));
        }
        _ => panic!("expected status result event"),
    }
}

#[tokio::test]
async fn worker_rejects_resolved_parameters_that_do_not_match_provider_metadata() {
    let mut command = action_command();
    command.action.provider = "console".into();
    command.action.function = "run".into();
    command.parameters = json!({ "command": 1 });

    let result = crate::executor::execute_task(
        &default_provider_factory(),
        std::sync::Arc::new(std::collections::HashMap::new()),
        command.action,
        command.workflow_node_run_id,
        command.parameters,
        None,
        runinator_plugin::cancel::CancellationToken::new(),
    )
    .await;

    assert_eq!(result.status, RunStatus::Failed);
    assert!(
        result
            .task_result
            .message
            .as_deref()
            .unwrap_or_default()
            .contains(
                "resolved action configuration 'console.run.command' expected string, got integer"
            )
    );
    assert!(result.execution_result.is_none());
}

#[tokio::test]
async fn worker_accepts_std_exec_program_with_context() {
    // the std `exec` action receives `{ program, context }` from the web service. its metadata must
    // declare both keys so the worker's closed-struct parameter validation accepts the context the
    // interpreter needs (regression: it once rejected `std.exec.context` as "not allowed").
    let action = WorkflowAction {
        provider: "std".into(),
        function: "exec".into(),
        timeout_seconds: 60,
        configuration: runinator_models::workflows::WorkflowObject::default(),
        mcp_enabled: false,
        tags: Vec::new(),
        required_labels: Default::default(),
    };
    let parameters = json!({
        "program": [ { "$return": { "ok": true } } ],
        "context": { "input": { "x": 1 } }
    });

    let result = crate::executor::execute_task(
        &default_provider_factory(),
        std::sync::Arc::new(std::collections::HashMap::new()),
        action,
        Uuid::new_v4(),
        parameters,
        None,
        runinator_plugin::cancel::CancellationToken::new(),
    )
    .await;

    assert_eq!(result.status, RunStatus::Succeeded);
    assert_eq!(
        result.execution_result.and_then(|r| r.output_json),
        Some(json!({ "ok": true }))
    );
}

#[tokio::test]
async fn worker_rejects_undeclared_std_exec_parameter() {
    // a key the `exec` action does not declare is still rejected, proving the context key above
    // passes because it is declared, not because validation is disabled for std.
    let action = WorkflowAction {
        provider: "std".into(),
        function: "exec".into(),
        timeout_seconds: 60,
        configuration: runinator_models::workflows::WorkflowObject::default(),
        mcp_enabled: false,
        tags: Vec::new(),
        required_labels: Default::default(),
    };
    let parameters = json!({
        "program": [ { "$return": true } ],
        "context": {},
        "bogus": 1
    });

    let result = crate::executor::execute_task(
        &default_provider_factory(),
        std::sync::Arc::new(std::collections::HashMap::new()),
        action,
        Uuid::new_v4(),
        parameters,
        None,
        runinator_plugin::cancel::CancellationToken::new(),
    )
    .await;

    assert_eq!(result.status, RunStatus::Failed);
    assert!(
        result
            .task_result
            .message
            .as_deref()
            .unwrap_or_default()
            .contains("is not allowed")
    );
}

#[test]
fn worker_validates_provider_output_fields_when_present() {
    let action_metadata =
        ActionMetadata::new("run", "run").with_results(vec![ResultMetadata::new(
            "exit_code",
            RuninatorType::Integer,
        )]);
    let action = WorkflowAction {
        provider: "console".into(),
        function: "run".into(),
        timeout_seconds: 60,
        configuration: runinator_models::workflows::WorkflowObject::default(),
        mcp_enabled: false,
        tags: Vec::new(),
        required_labels: Default::default(),
    };
    let result = TaskExecutionResult {
        message: None,
        output_json: Some(json!({ "exit_code": "zero" })),
        chunks: Vec::new(),
        artifacts: Vec::new(),
    };

    let err = crate::executor::validate_execution_result(&action_metadata, &action, &result)
        .expect_err("typed result field is validated");
    assert!(err.contains("provider output 'console.run.exit_code' expected integer, got string"));
}

fn action_command() -> ActionCommand {
    ActionCommand {
        command_id: Uuid::new_v4(),
        workflow_run_id: Uuid::new_v4(),
        workflow_node_run_id: Uuid::new_v4(),
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
        target: Default::default(),
        trace_id: Uuid::nil(),
        trace_context: Default::default(),
    }
}

fn test_config() -> Config {
    Config {
        dll_paths: Vec::new(),
        broker_backend: "in-memory".into(),
        broker_endpoint: "127.0.0.1:7070".into(),
        broker_action_topic: "runinator.actions".into(),
        broker_control_topic: "runinator.control".into(),
        broker_result_topic: "runinator.results".into(),
        broker_client_id: "test-worker".into(),
        broker_consumer_id: "test-consumer".into(),
        max_concurrent_actions: 1,
        shutdown_grace_seconds: 30,
        api_base_url: "http://127.0.0.1:8080/".into(),
        api_key: None,
        worker_id: Uuid::new_v4(),
        advertise_host: None,
        liveness_file: String::new(),
        labels: Default::default(),
    }
}
