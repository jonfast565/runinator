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

    let err = match build_broker(&config.broker_config()).await {
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

    let err = match build_broker(&config.broker_config()).await {
        Ok(_) => panic!("expected rabbitmq result channel startup guard to fail"),
        Err(err) => err,
    };

    assert!(err.to_string().contains("Broker backend 'rabbitmq'"));
    assert!(err.to_string().contains("non-empty workflow result queue"));
}

// only meaningful when the relay transport is compiled in; without the feature build_broker
// fails fast with a feature-disabled error (covered below).
#[cfg(feature = "ws")]
#[tokio::test]
async fn build_broker_supports_relaying_through_the_ws_backend() {
    // proves any worker (not just runinator-desktop-agent) can pick "connect through the
    // runinator-ws relay" instead of a direct broker backend, via the same config/build_broker path.
    let mut config = test_config();
    config.broker_backend = "ws".into();
    config.broker_endpoint = "ws://127.0.0.1:0/ws/desktop-worker".into();

    build_broker(&config.broker_config())
        .await
        .expect("the ws backend should build even before any connection attempt completes");
}

#[cfg(not(feature = "ws"))]
#[tokio::test]
async fn build_broker_rejects_the_ws_backend_when_the_feature_is_compiled_out() {
    let mut config = test_config();
    config.broker_backend = "ws".into();
    config.broker_endpoint = "ws://127.0.0.1:0/ws/desktop-worker".into();

    let err = match build_broker(&config.broker_config()).await {
        Ok(_) => panic!("expected the ws backend to be rejected without the `ws` feature"),
        Err(err) => err,
    };

    assert!(err.to_string().contains("ws"));
    assert!(err.to_string().contains("feature"));
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

// a provider whose execution blocks until its cancellation token fires, flagging when it starts.
struct BlockingProvider {
    started: std::sync::Arc<std::sync::atomic::AtomicBool>,
}

impl runinator_plugin::provider::Provider for BlockingProvider {
    fn name(&self) -> String {
        "test".into()
    }

    fn metadata(&self) -> runinator_models::providers::ProviderMetadata {
        runinator_models::providers::ProviderMetadata {
            name: "test".into(),
            actions: vec![ActionMetadata::new("execute", "blocks until canceled")],
            metadata: Default::default(),
        }
    }

    fn execute_service(
        &self,
        _request: runinator_models::runs::ProviderExecutionRequest,
        _sink: Option<std::sync::Arc<dyn runinator_plugin::provider::ProviderEventSink>>,
        token: runinator_plugin::cancel::CancellationToken,
    ) -> Result<TaskExecutionResult, runinator_models::errors::SendableError> {
        self.started
            .store(true, std::sync::atomic::Ordering::SeqCst);
        while !token.is_cancelled() {
            std::thread::sleep(std::time::Duration::from_millis(20));
        }
        // linger after the cancel so the executor's cancel arm settles the outcome as `Canceled`
        // deterministically, then return so the blocking thread does not outlive the test runtime.
        std::thread::sleep(std::time::Duration::from_millis(500));
        Err("canceled".into())
    }
}

// a worker loop harness executing a single blocking action against an in-memory broker. the api
// endpoint is unreachable and replica_id is unset, so no executor-claim traffic occurs.
fn blocking_worker_runtime(
    broker: std::sync::Arc<InMemoryBroker>,
    started: std::sync::Arc<std::sync::atomic::AtomicBool>,
    shutdown: std::sync::Arc<tokio::sync::Notify>,
) -> crate::worker::WorkerRuntime {
    blocking_worker_runtime_with_concurrency(broker, started, shutdown, 1)
}

fn blocking_worker_runtime_with_concurrency(
    broker: std::sync::Arc<InMemoryBroker>,
    started: std::sync::Arc<std::sync::atomic::AtomicBool>,
    shutdown: std::sync::Arc<tokio::sync::Notify>,
    max_concurrent_actions: usize,
) -> crate::worker::WorkerRuntime {
    crate::worker::WorkerRuntime {
        broker,
        profile: runinator_comm::ConsumerProfile::shared("test-consumer"),
        libraries: std::sync::Arc::new(Default::default()),
        api_client: runinator_api::AsyncApiClient::with_credentials(
            runinator_api::StaticLocator::new("http://127.0.0.1:9/"),
            None,
        )
        .unwrap(),
        replica_id: None,
        providers: std::sync::Arc::new(move || {
            vec![Box::new(BlockingProvider {
                started: started.clone(),
            }) as runinator_provider_catalog::StaticProvider]
        }),
        max_concurrent_actions,
        shutdown_grace: std::time::Duration::from_secs(5),
        shutdown,
        events: std::sync::Arc::new(crate::events::NoopEventSink),
    }
}

async fn wait_until_started(started: &std::sync::atomic::AtomicBool) {
    tokio::time::timeout(std::time::Duration::from_secs(5), async {
        while !started.load(std::sync::atomic::Ordering::SeqCst) {
            tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        }
    })
    .await
    .expect("provider should start executing");
}

#[tokio::test]
async fn shutdown_preempted_action_is_requeued_not_canceled() {
    let broker = std::sync::Arc::new(InMemoryBroker::new());
    let command = action_command();
    broker
        .publish(runinator_broker::BrokerMessage {
            command: command.clone(),
            dedupe_key: None,
            enqueued_at: chrono::Utc::now(),
        })
        .await
        .unwrap();

    let started = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
    let shutdown = std::sync::Arc::new(tokio::sync::Notify::new());
    let runtime = blocking_worker_runtime(broker.clone(), started.clone(), shutdown.clone());
    let worker = tokio::spawn(crate::worker::start_worker_loop(runtime));

    wait_until_started(&started).await;
    shutdown.notify_waiters();
    worker.await.unwrap().unwrap();

    // the run was not canceled: no terminal status may be published, only the initial `Running`.
    let running = broker.receive_result("test-ws").await.unwrap();
    match running.event.kind {
        WorkflowResultEventKind::Status { status, .. } => {
            assert_eq!(status, WorkflowStatus::Running)
        }
        other => panic!("expected running status, got {other:?}"),
    }
    assert!(
        tokio::time::timeout(
            std::time::Duration::from_millis(200),
            broker.receive_result("test-ws"),
        )
        .await
        .is_err(),
        "a shutdown-preempted action must not publish a terminal status"
    );

    // the delivery must be back on the action channel for another worker to pick up.
    let redelivered = tokio::time::timeout(
        std::time::Duration::from_secs(2),
        broker.receive_for(&runinator_comm::ConsumerProfile::shared("verify")),
    )
    .await
    .expect("preempted delivery should be redelivered")
    .unwrap();
    assert_eq!(
        redelivered.command.workflow_node_run_id,
        command.workflow_node_run_id
    );
}

#[tokio::test]
async fn control_canceled_action_still_publishes_canceled_status() {
    let broker = std::sync::Arc::new(InMemoryBroker::new());
    let command = action_command();
    broker
        .publish(runinator_broker::BrokerMessage {
            command: command.clone(),
            dedupe_key: None,
            enqueued_at: chrono::Utc::now(),
        })
        .await
        .unwrap();

    let started = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
    let shutdown = std::sync::Arc::new(tokio::sync::Notify::new());
    let runtime = blocking_worker_runtime(broker.clone(), started.clone(), shutdown.clone());
    let worker = tokio::spawn(crate::worker::start_worker_loop(runtime));

    wait_until_started(&started).await;
    broker
        .publish_control(runinator_comm::ControlCommand::for_node_run(
            command.workflow_run_id,
            command.workflow_node_run_id,
            runinator_comm::ControlKind::Cancel,
        ))
        .await
        .unwrap();

    // a genuine cancel settles the node as canceled: expect the terminal status on the result
    // channel (skipping the initial running status and any log chunks).
    let canceled_seen = tokio::time::timeout(std::time::Duration::from_secs(5), async {
        loop {
            let delivery = broker.receive_result("test-ws").await.unwrap();
            if let WorkflowResultEventKind::Status { status, .. } = delivery.event.kind
                && status == WorkflowStatus::Canceled
            {
                return;
            }
        }
    })
    .await;
    assert!(
        canceled_seen.is_ok(),
        "a control-requested cancel must publish a canceled status"
    );

    shutdown.notify_waiters();
    worker.await.unwrap().unwrap();

    // the canceled delivery was acked, not requeued.
    assert!(
        tokio::time::timeout(
            std::time::Duration::from_millis(200),
            broker.receive_for(&runinator_comm::ConsumerProfile::shared("verify")),
        )
        .await
        .is_err(),
        "a control-canceled delivery must be acked, not redelivered"
    );
}

#[tokio::test]
async fn duplicate_delivery_of_in_flight_node_run_is_acked_without_executing() {
    let broker = std::sync::Arc::new(InMemoryBroker::new());
    let command = action_command();
    // two deliveries of the same node run (a timeout-raced duplicate carries a fresh command id);
    // the loser of the in-flight race must be dropped.
    for _ in 0..2 {
        let mut duplicate = command.clone();
        duplicate.command_id = uuid::Uuid::new_v4();
        broker
            .publish(runinator_broker::BrokerMessage {
                command: duplicate,
                dedupe_key: None,
                enqueued_at: chrono::Utc::now(),
            })
            .await
            .unwrap();
    }

    let started = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
    let shutdown = std::sync::Arc::new(tokio::sync::Notify::new());
    let runtime = blocking_worker_runtime_with_concurrency(
        broker.clone(),
        started.clone(),
        shutdown.clone(),
        2,
    );
    let worker = tokio::spawn(crate::worker::start_worker_loop(runtime));

    wait_until_started(&started).await;
    // let the duplicate work through its (ack-and-drop) path before settling the winner.
    tokio::time::sleep(std::time::Duration::from_millis(200)).await;
    broker
        .publish_control(runinator_comm::ControlCommand::for_node_run(
            command.workflow_run_id,
            command.workflow_node_run_id,
            runinator_comm::ControlKind::Cancel,
        ))
        .await
        .unwrap();

    // drain result events until the terminal status: exactly one running status may appear, so the
    // duplicate never reached the observable execution path.
    let mut running_count = 0;
    tokio::time::timeout(std::time::Duration::from_secs(5), async {
        loop {
            let delivery = broker.receive_result("test-ws").await.unwrap();
            if let WorkflowResultEventKind::Status { status, .. } = delivery.event.kind {
                if status == WorkflowStatus::Running {
                    running_count += 1;
                } else if status.is_terminal() {
                    return;
                }
            }
        }
    })
    .await
    .expect("the surviving execution should settle terminally");
    assert_eq!(
        running_count, 1,
        "the duplicate delivery must not start executing"
    );

    shutdown.notify_waiters();
    worker.await.unwrap().unwrap();

    // both deliveries settled by ack: nothing is left to redeliver.
    assert!(
        tokio::time::timeout(
            std::time::Duration::from_millis(200),
            broker.receive_for(&runinator_comm::ConsumerProfile::shared("verify")),
        )
        .await
        .is_err(),
        "a dropped duplicate must be acked, not redelivered"
    );
}

#[tokio::test]
async fn own_stale_leases_match_only_at_or_past_the_recorded_attempt() {
    let leases = crate::worker::OwnStaleLeases::default();
    let node_run_id = uuid::Uuid::new_v4();
    leases.record(node_run_id, 2).await;
    // an older attempt must never take the lease back (it was superseded).
    assert!(!leases.matches(node_run_id, 1).await);
    assert!(leases.matches(node_run_id, 2).await);
    assert!(leases.matches(node_run_id, 3).await);
    assert!(!leases.matches(uuid::Uuid::new_v4(), 2).await);
    leases.clear(node_run_id).await;
    assert!(!leases.matches(node_run_id, 2).await);
}

#[test]
fn secret_resolution_errors_classify_transient_vs_definitive() {
    use crate::secrets::is_transient_secret_error;
    use runinator_models::errors::SendableError;

    let server_error: SendableError = Box::new(runinator_api::ApiError::Http {
        status: reqwest::StatusCode::INTERNAL_SERVER_ERROR,
        url: "http://ws.local/".parse().unwrap(),
        message: "boom".into(),
    });
    assert!(is_transient_secret_error(&server_error));

    let missing: SendableError = Box::new(runinator_api::ApiError::Http {
        status: reqwest::StatusCode::NOT_FOUND,
        url: "http://ws.local/".parse().unwrap(),
        message: "no such secret".into(),
    });
    assert!(!is_transient_secret_error(&missing));

    let malformed: SendableError = Box::new(runinator_api::ApiError::UnexpectedResponse(
        "not json".into(),
    ));
    assert!(!is_transient_secret_error(&malformed));

    let unrelated: SendableError = "plain".into();
    assert!(!is_transient_secret_error(&unrelated));
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
