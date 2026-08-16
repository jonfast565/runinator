//! the web service’s behaviour tests, partitioned by subject.
//!
//! most of these boot a real `SqliteDb` and drive the engine through it, because what they cover is
//! the seam between an http handler, the reducer, and persistence — the part no single crate owns.
//! prefer `runinator-reducer`’s fake-backed suite for anything that is purely a node-handler
//! decision; reach for this layer when the database or the broker is part of the assertion.
//!
//! shared fixtures live here so each submodule picks them up through its `use super` glob.

mod authz;
mod bootstrap;
mod chaining;
mod console;
mod control_flow;
mod correlation;
#[cfg(feature = "ws")]
mod desktop_relay;
mod functions;
mod models;
mod orgs;
mod packs;
mod reducer;
mod result_consumer;
mod revisions;
mod runs;
mod users;
mod validation;
mod wdl;

use std::{
    collections::HashSet,
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};

use axum::{Extension, Json, extract::Path, http::StatusCode};
use runinator_broker::{
    Broker, BrokerDelivery, BrokerError, BrokerMessage, ControlCommand, ControlDelivery,
    EventDelivery, EventMessage, IngressDelivery, IngressMessage, ResultDelivery, ResultMessage,
    WakeDelivery, WakeMessage, in_memory::InMemoryBroker,
};
use runinator_comm::{ActionCommand, WorkflowResultEvent};
use runinator_database::{
    BootstrapOptions, bootstrap_database, interfaces::prelude::*, load_jwt_secret,
    seed_bootstrap_admin, seed_bootstrap_service_api_key, sqlite::SqliteDb,
};
use runinator_models::json;
use runinator_models::value::Value;
use runinator_models::{
    auth::{
        AuthContext, CreateApiKeyRequest, Grant, Permission, PrincipalKind, PrincipalType,
        ResourceType, UpdateApiKeyRequest, UpdateUserRequest,
    },
    orgs::{
        AddOrgMemberRequest, CreateOrgRequest, OrgRole, UpdateOrgMemberRequest, UpdateOrgRequest,
    },
    revisions::{RevisionAuthor, RevisionSource},
    runs::{NewRunArtifact, NewRunChunk},
    workflows::{
        NewWorkflowRunArtifact, WorkflowAction, WorkflowBundle, WorkflowDefinition, WorkflowGraph,
        WorkflowNodeRun, WorkflowStatus, WorkflowTrigger, WorkflowTriggerKind,
    },
};
use runinator_wdl::WdlFragmentKind;
use runinator_workflows::{WorkflowTypeDiagnostic, WorkflowValidationError};
use runinator_ws_middleware::authz::{AuthContextExt, AuthzChecker};
use tokio::sync::Notify;
use uuid::Uuid;

async fn test_db() -> (SqliteDb, std::path::PathBuf) {
    let path = std::env::temp_dir().join(format!(
        "runinator-ws-workflows-{}.db",
        uuid::Uuid::new_v4()
    ));
    let db = SqliteDb::new(path.to_str().unwrap()).await.unwrap();
    db.run_init_scripts(&Vec::new()).await.unwrap();
    (db, path)
}

/// save a workflow the way a test wants to: attributed to the platform rather than to a caller.
/// the revision-recording path still runs, so tests exercise it without restating an author.
async fn save_workflow<T: runinator_database::interfaces::DatabaseImpl>(
    db: &T,
    workflow: &WorkflowDefinition,
) -> Result<WorkflowDefinition, runinator_models::errors::SendableError> {
    crate::repository::upsert_workflow(
        db,
        workflow,
        &runinator_models::revisions::RevisionAuthor::system(
            runinator_models::revisions::RevisionSource::Api,
        ),
    )
    .await
}

async fn create_node_run(db: &SqliteDb) -> WorkflowNodeRun {
    let workflow = save_workflow(db, &workflow(None, "result-consumer"))
        .await
        .unwrap();
    let workflow_id = workflow.id.unwrap();
    let run = crate::repository::create_workflow_run(
        db,
        workflow_id,
        json!({}),
        false,
        None,
        Default::default(),
    )
    .await
    .unwrap();
    crate::repository::update_workflow_run_status(
        db,
        run.id,
        WorkflowStatus::Running,
        Some("start".into()),
        None,
        None,
    )
    .await
    .unwrap();
    crate::repository::create_workflow_node_run(db, run.id, "node-a".into(), json!({}), None)
        .await
        .unwrap()
}

fn action_command(
    workflow_run_id: Uuid,
    workflow_node_run_id: Uuid,
    node_id: &str,
) -> ActionCommand {
    ActionCommand {
        command_id: Uuid::new_v4(),
        workflow_run_id,
        workflow_node_run_id,
        node_id: node_id.into(),
        action: WorkflowAction {
            provider: "test".into(),
            function: "execute".into(),
            timeout_seconds: 60,
            configuration: runinator_models::workflows::WorkflowObject::default(),
            mcp_enabled: false,
            tags: Vec::new(),
            required_labels: Default::default(),
            idempotency_key: None,
            function_binding: None,
        },
        attempt: 1,
        parameters: json!({}),
        target: Default::default(),
        trace_id: Uuid::nil(),
        trace_context: Default::default(),
        notification_delivery_id: None,
        invocation_call_id: None,
        idempotency_key: None,
    }
}

async fn publish_duplicate_results(broker: &RecordingBroker, events: &[WorkflowResultEvent]) {
    for event in events {
        for duplicate in 0..2 {
            broker
                .publish_result(ResultMessage {
                    event: event.clone(),
                    dedupe_key: Some(format!("{}-{duplicate}", event.event_id)),
                    enqueued_at: chrono::Utc::now(),
                })
                .await
                .unwrap();
        }
    }
}

async fn wait_until(mut condition: impl FnMut() -> bool) {
    let deadline = Instant::now() + Duration::from_secs(2);
    while Instant::now() < deadline {
        if condition() {
            return;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    assert!(condition(), "condition was not met before timeout");
}

#[derive(Clone, Default)]
struct RecordingBroker {
    inner: InMemoryBroker,
    result_receives: Arc<Mutex<HashSet<Uuid>>>,
    result_acks: Arc<Mutex<HashSet<Uuid>>>,
    result_nacks: Arc<Mutex<HashSet<Uuid>>>,
}

impl RecordingBroker {
    fn new() -> Self {
        Self::default()
    }

    fn result_receives(&self) -> HashSet<Uuid> {
        self.result_receives.lock().unwrap().clone()
    }

    fn result_acks(&self) -> HashSet<Uuid> {
        self.result_acks.lock().unwrap().clone()
    }

    fn result_nacks(&self) -> HashSet<Uuid> {
        self.result_nacks.lock().unwrap().clone()
    }
}

#[async_trait::async_trait]
impl Broker for RecordingBroker {
    async fn publish(&self, message: BrokerMessage) -> Result<(), BrokerError> {
        self.inner.publish(message).await
    }

    async fn receive(&self, consumer: &str) -> Result<BrokerDelivery, BrokerError> {
        self.inner.receive(consumer).await
    }

    async fn ack(&self, consumer: &str, delivery_id: Uuid) -> Result<(), BrokerError> {
        self.inner.ack(consumer, delivery_id).await
    }

    async fn nack(&self, consumer: &str, delivery_id: Uuid) -> Result<(), BrokerError> {
        self.inner.nack(consumer, delivery_id).await
    }

    async fn publish_control(&self, command: ControlCommand) -> Result<(), BrokerError> {
        self.inner.publish_control(command).await
    }

    async fn receive_control(&self, consumer: &str) -> Result<ControlDelivery, BrokerError> {
        self.inner.receive_control(consumer).await
    }

    async fn ack_control(&self, consumer: &str, delivery_id: Uuid) -> Result<(), BrokerError> {
        self.inner.ack_control(consumer, delivery_id).await
    }

    async fn nack_control(&self, consumer: &str, delivery_id: Uuid) -> Result<(), BrokerError> {
        self.inner.nack_control(consumer, delivery_id).await
    }

    async fn publish_result(&self, message: ResultMessage) -> Result<(), BrokerError> {
        self.inner.publish_result(message).await
    }

    async fn receive_result(&self, consumer: &str) -> Result<ResultDelivery, BrokerError> {
        let delivery = self.inner.receive_result(consumer).await?;
        self.result_receives
            .lock()
            .unwrap()
            .insert(delivery.delivery_id);
        Ok(delivery)
    }

    async fn ack_result(&self, consumer: &str, delivery_id: Uuid) -> Result<(), BrokerError> {
        self.inner.ack_result(consumer, delivery_id).await?;
        self.result_acks.lock().unwrap().insert(delivery_id);
        Ok(())
    }

    async fn nack_result(&self, consumer: &str, delivery_id: Uuid) -> Result<(), BrokerError> {
        self.inner.nack_result(consumer, delivery_id).await?;
        self.result_nacks.lock().unwrap().insert(delivery_id);
        Ok(())
    }

    async fn publish_wake(&self, message: WakeMessage) -> Result<(), BrokerError> {
        self.inner.publish_wake(message).await
    }

    async fn receive_wake(&self, consumer: &str) -> Result<WakeDelivery, BrokerError> {
        self.inner.receive_wake(consumer).await
    }

    async fn ack_wake(&self, consumer: &str, delivery_id: Uuid) -> Result<(), BrokerError> {
        self.inner.ack_wake(consumer, delivery_id).await
    }

    async fn nack_wake(&self, consumer: &str, delivery_id: Uuid) -> Result<(), BrokerError> {
        self.inner.nack_wake(consumer, delivery_id).await
    }

    async fn publish_ingress(&self, message: IngressMessage) -> Result<(), BrokerError> {
        self.inner.publish_ingress(message).await
    }

    async fn receive_ingress(&self, consumer: &str) -> Result<IngressDelivery, BrokerError> {
        self.inner.receive_ingress(consumer).await
    }

    async fn ack_ingress(&self, consumer: &str, delivery_id: Uuid) -> Result<(), BrokerError> {
        self.inner.ack_ingress(consumer, delivery_id).await
    }

    async fn nack_ingress(&self, consumer: &str, delivery_id: Uuid) -> Result<(), BrokerError> {
        self.inner.nack_ingress(consumer, delivery_id).await
    }

    async fn publish_event(&self, message: EventMessage) -> Result<(), BrokerError> {
        self.inner.publish_event(message).await
    }

    async fn receive_event(&self, consumer: &str) -> Result<EventDelivery, BrokerError> {
        self.inner.receive_event(consumer).await
    }
}

fn workflow(id: Option<Uuid>, name: &str) -> WorkflowDefinition {
    WorkflowDefinition {
        id,
        name: name.into(),
        namespace: None,
        org_id: None,
        version: runinator_models::semver::SemVer::new(1, 0, 0),
        enabled: true,
        input_type: runinator_models::types::RuninatorType::from_json_schema(
            &json!({ "type": "object" }),
        ),
        definition: WorkflowGraph::from_value(json!({
            "start": "start",
            "nodes": [
                { "id": "start", "kind": "start", "transitions": { "next": { "$node": "done" } } },
                { "id": "done", "kind": "end" }
            ]
        }))
        .unwrap(),
        created_at: None,
        updated_at: None,
    }
}

fn user_ctx(user_id: Uuid) -> AuthContext {
    AuthContext {
        principal_id: Some(user_id),
        is_admin: false,
        kind: PrincipalKind::User,
        org_id: None,
        org_role: None,
    }
}

fn grant(
    workflow_id: Uuid,
    principal_type: PrincipalType,
    principal_id: Uuid,
    permission: Permission,
) -> Grant {
    Grant {
        id: None,
        resource_type: ResourceType::Workflow,
        resource_id: workflow_id,
        principal_type,
        principal_id,
        permission,
        created_at: chrono::Utc::now(),
    }
}
fn trigger(id: Option<Uuid>, workflow_id: Uuid) -> WorkflowTrigger {
    WorkflowTrigger {
        id,
        workflow_id,
        kind: WorkflowTriggerKind::Manual,
        enabled: true,
        configuration: json!({}),
        next_execution: None,
        blackout_start: None,
        blackout_end: None,
        metadata: json!({}),
        created_at: None,
        updated_at: None,
    }
}

/// claim and process every currently-ready node until the queue drains.
async fn drain_ready_nodes(db: &SqliteDb) {
    for _ in 0..256 {
        let ready = crate::repository::claim_ready_nodes(
            db,
            "test".into(),
            chrono::Utc::now() + chrono::Duration::seconds(30),
            50,
        )
        .await
        .unwrap();
        if ready.is_empty() {
            break;
        }
        for node in ready {
            crate::repository::complete_ready_node(db, node.id, "test".into(), None)
                .await
                .unwrap();
        }
    }
}

/// drive a run to a terminal state, simulating workers that succeed every dispatched action.
async fn run_to_completion(
    db: &SqliteDb,
    run_id: Uuid,
) -> runinator_models::workflows::WorkflowRun {
    for _ in 0..64 {
        drain_ready_nodes(db).await;
        let (run, _) = crate::repository::fetch_workflow_run(db, run_id)
            .await
            .unwrap()
            .unwrap();
        if run.status.is_terminal() {
            return run;
        }
        let dispatches = db.fetch_pending_action_dispatches(50).await.unwrap();
        if dispatches.is_empty() {
            // parked on something with no pending action (e.g. an unresolved approval).
            return run;
        }
        for dispatch in dispatches {
            db.mark_action_dispatch_published(dispatch.id)
                .await
                .unwrap();
            let event = WorkflowResultEvent::status(
                &dispatch.command,
                WorkflowStatus::Succeeded,
                Some(json!({ "ok": true })),
                None,
            );
            crate::repository::apply_workflow_result_event(db, &event)
                .await
                .unwrap();
        }
    }
    let (run, _) = crate::repository::fetch_workflow_run(db, run_id)
        .await
        .unwrap()
        .unwrap();
    run
}

async fn seed_run(db: &SqliteDb, name: &str, definition: Value) -> Uuid {
    let mut workflow = workflow(None, name);
    workflow.definition = WorkflowGraph::from_value(definition).unwrap();
    let workflow = db.upsert_workflow(&workflow).await.unwrap();
    crate::repository::create_workflow_run(
        db,
        workflow.id.unwrap(),
        json!({}),
        false,
        None,
        Default::default(),
    )
    .await
    .unwrap()
    .id
}
