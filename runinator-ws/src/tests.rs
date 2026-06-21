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
    BootstrapOptions, bootstrap_database, interfaces::DatabaseImpl, load_jwt_secret,
    seed_bootstrap_admin, seed_bootstrap_service_api_key, sqlite::SqliteDb,
};
use runinator_models::json;
use runinator_models::value::Value;
use runinator_models::{
    auth::{
        AuthContext, CreateApiKeyRequest, Grant, Permission, PrincipalKind, PrincipalType,
        ResourceType, UpdateApiKeyRequest, UpdateUserRequest,
    },
    runs::{NewRunArtifact, NewRunChunk},
    workflows::{
        NewWorkflowRunDeliverable, WorkflowAction, WorkflowBundle, WorkflowDefinition,
        WorkflowGraph, WorkflowNodeRun, WorkflowStatus, WorkflowTrigger, WorkflowTriggerKind,
    },
};
use runinator_wdl::WdlFragmentKind;
use runinator_workflows::{WorkflowTypeDiagnostic, WorkflowValidationError};
use tokio::sync::Notify;
use uuid::Uuid;

#[test]
fn workflow_run_stream_terminal_status_stays_snapshot_message() {
    let response = crate::models::WorkflowRunResponse {
        run: runinator_models::workflows::WorkflowRun {
            id: Uuid::now_v7(),
            workflow_id: Uuid::now_v7(),
            workflow_snapshot: None,
            status: runinator_models::workflows::WorkflowStatus::Succeeded,
            active_node_id: None,
            parameters: json!({}),
            state: json!({}),
            created_at: chrono::Utc::now(),
            started_at: None,
            finished_at: Some(chrono::Utc::now()),
            message: None,
            name: None,
            trigger_source_kind: None,
            trigger_actor_type: None,
            trigger_actor_replica_id: None,
            trigger_actor_display_name: None,
            trigger_request_host: None,
            trigger_request_ip: None,
            trigger_metadata: Value::Null,
        },
        nodes: vec![],
    };

    let value: Value = serde_json::to_value(response).unwrap().into();

    assert_eq!(value["run"]["status"], "succeeded");
    assert_eq!(value["nodes"], json!([]));
    assert!(value.get("type").is_none());
}

#[test]
fn workflow_run_request_defaults_to_non_debug() {
    let request: crate::models::WorkflowRunRequest =
        serde_json::from_value(json!({ "parameters": { "mode": "test" } }).into()).unwrap();

    assert!(!request.debug);
    assert_eq!(request.parameters["mode"], "test");
}

#[test]
fn workflow_run_request_accepts_debug_flag() {
    let request: crate::models::WorkflowRunRequest =
        serde_json::from_value(json!({ "parameters": {}, "debug": true }).into()).unwrap();

    assert!(request.debug);
}

#[tokio::test]
async fn seed_bootstrap_admin_creates_local_admin_credentials() {
    let (db, path) = test_db().await;

    seed_bootstrap_admin(&db, "admin:secret-pass", false)
        .await
        .unwrap();

    let user = db
        .fetch_user_by_username("admin".into())
        .await
        .unwrap()
        .expect("seeded user");
    let credential = db
        .fetch_local_credential("admin".into())
        .await
        .unwrap()
        .expect("seeded credential");

    assert!(user.is_admin);
    assert_eq!(db.count_users().await.unwrap(), 1);
    assert_eq!(credential.user.id, user.id);
    assert!(crate::auth::verify_password(
        "secret-pass",
        &credential.password_hash
    ));

    let _ = std::fs::remove_file(path);
}

#[tokio::test]
async fn seed_bootstrap_admin_does_not_overwrite_existing_users() {
    let (db, path) = test_db().await;

    db.create_user("existing".into(), None, false, None)
        .await
        .unwrap();

    seed_bootstrap_admin(&db, "admin:secret-pass", false)
        .await
        .unwrap();

    assert_eq!(db.count_users().await.unwrap(), 1);
    assert!(
        db.fetch_user_by_username("admin".into())
            .await
            .unwrap()
            .is_none()
    );

    let _ = std::fs::remove_file(path);
}

#[tokio::test]
async fn seed_bootstrap_admin_force_resets_existing_admin() {
    let (db, path) = test_db().await;

    seed_bootstrap_admin(&db, "admin:old-pass", false)
        .await
        .unwrap();
    // a non-force re-seed must not touch the existing admin.
    seed_bootstrap_admin(&db, "admin:new-pass", false)
        .await
        .unwrap();
    let credential = db
        .fetch_local_credential("admin".into())
        .await
        .unwrap()
        .expect("credential");
    assert!(crate::auth::verify_password(
        "old-pass",
        &credential.password_hash
    ));

    // force reconciles the stale password without creating a duplicate user.
    seed_bootstrap_admin(&db, "admin:new-pass", true)
        .await
        .unwrap();
    let credential = db
        .fetch_local_credential("admin".into())
        .await
        .unwrap()
        .expect("credential");
    assert_eq!(db.count_users().await.unwrap(), 1);
    assert!(crate::auth::verify_password(
        "new-pass",
        &credential.password_hash
    ));
    assert!(!crate::auth::verify_password(
        "old-pass",
        &credential.password_hash
    ));

    let _ = std::fs::remove_file(path);
}

#[tokio::test]
async fn seed_bootstrap_admin_force_provisions_alongside_existing_users() {
    let (db, path) = test_db().await;

    db.create_user("existing".into(), None, false, None)
        .await
        .unwrap();
    seed_bootstrap_admin(&db, "admin:secret-pass", true)
        .await
        .unwrap();

    let user = db
        .fetch_user_by_username("admin".into())
        .await
        .unwrap()
        .expect("seeded admin");
    assert!(user.is_admin);
    assert_eq!(db.count_users().await.unwrap(), 2);

    let _ = std::fs::remove_file(path);
}

#[tokio::test]
async fn seed_bootstrap_service_api_key_creates_admin_service_key() {
    let (db, path) = test_db().await;
    let raw_key = "localdev.runinator-local-dev-service-key";

    seed_bootstrap_service_api_key(&db, "local-dev", raw_key)
        .await
        .unwrap();

    let record = db
        .fetch_api_key_by_prefix("localdev".into())
        .await
        .unwrap()
        .expect("seeded api key");

    assert_eq!(record.key.name, "local-dev");
    assert!(record.key.is_service);
    assert!(record.is_admin);
    assert_eq!(record.key_hash, crate::auth::hash_secret(raw_key));

    let _ = std::fs::remove_file(path);
}

#[tokio::test]
async fn seed_bootstrap_service_api_key_is_idempotent_for_existing_prefix() {
    let (db, path) = test_db().await;
    let raw_key = "localdev.runinator-local-dev-service-key";

    seed_bootstrap_service_api_key(&db, "local-dev", raw_key)
        .await
        .unwrap();
    seed_bootstrap_service_api_key(&db, "local-dev", raw_key)
        .await
        .unwrap();

    assert_eq!(db.list_api_keys(None).await.unwrap().len(), 1);

    let _ = std::fs::remove_file(path);
}

#[tokio::test]
async fn user_admin_handlers_preserve_last_enabled_admin() {
    let (db, path) = test_db().await;
    let db = Arc::new(db);
    let admin = db
        .create_user("admin".into(), None, true, None)
        .await
        .unwrap();
    let admin_id = admin.id.expect("admin id");
    let ctx = AuthContext {
        principal_id: Some(admin_id),
        is_admin: true,
        kind: PrincipalKind::User,
    };

    let (status, _) = crate::handlers::auth::update_user::<SqliteDb>(
        Extension(db.clone()),
        Extension(ctx.clone()),
        Path(admin_id),
        Json(UpdateUserRequest {
            email: None,
            password: None,
            is_admin: Some(false),
            disabled: None,
        }),
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN);

    let (status, _) = crate::handlers::auth::update_user::<SqliteDb>(
        Extension(db.clone()),
        Extension(ctx.clone()),
        Path(admin_id),
        Json(UpdateUserRequest {
            email: None,
            password: None,
            is_admin: None,
            disabled: Some(true),
        }),
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN);

    let (status, _) = crate::handlers::auth::delete_user::<SqliteDb>(
        Extension(db.clone()),
        Extension(ctx),
        Path(admin_id),
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN);

    let second = db
        .create_user("second".into(), None, true, None)
        .await
        .unwrap();
    let (status, _) = crate::handlers::auth::update_user::<SqliteDb>(
        Extension(db.clone()),
        Extension(AuthContext {
            principal_id: second.id,
            is_admin: true,
            kind: PrincipalKind::User,
        }),
        Path(admin_id),
        Json(UpdateUserRequest {
            email: None,
            password: None,
            is_admin: Some(false),
            disabled: None,
        }),
    )
    .await;
    assert_eq!(status, StatusCode::OK);

    let _ = std::fs::remove_file(path);
}

#[tokio::test]
async fn api_key_handlers_support_admin_user_keys_and_rotation() {
    let (db, path) = test_db().await;
    let db = Arc::new(db);
    let admin = db
        .create_user("admin".into(), None, true, None)
        .await
        .unwrap();
    let user = db
        .create_user("operator".into(), None, false, None)
        .await
        .unwrap();
    let admin_ctx = AuthContext {
        principal_id: admin.id,
        is_admin: true,
        kind: PrincipalKind::User,
    };
    let user_id = user.id.expect("user id");

    let (status, _) = crate::handlers::auth::create_api_key::<SqliteDb>(
        Extension(db.clone()),
        Extension(admin_ctx.clone()),
        Json(CreateApiKeyRequest {
            name: "operator key".into(),
            user_id: Some(user_id),
            is_service: false,
            expires_at: None,
        }),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let keys = db.list_api_keys(Some(user_id)).await.unwrap();
    assert_eq!(keys.len(), 1);
    assert_eq!(keys[0].name, "operator key");
    assert!(!keys[0].is_service);

    let key_id = keys[0].id.expect("key id");
    let (status, _) = crate::handlers::auth::update_api_key::<SqliteDb>(
        Extension(db.clone()),
        Extension(admin_ctx.clone()),
        Path(key_id),
        Json(UpdateApiKeyRequest {
            name: Some("renamed key".into()),
            expires_at: Some(None),
            disabled: Some(false),
        }),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let updated = db.fetch_api_key(key_id).await.unwrap().unwrap();
    assert_eq!(updated.key.name, "renamed key");

    let (status, _) = crate::handlers::auth::rotate_api_key::<SqliteDb>(
        Extension(db.clone()),
        Extension(admin_ctx),
        Path(key_id),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let old = db.fetch_api_key(key_id).await.unwrap().unwrap();
    assert!(old.key.disabled);
    let keys = db.list_api_keys(Some(user_id)).await.unwrap();
    assert_eq!(keys.len(), 2);
    assert_eq!(keys.iter().filter(|key| !key.disabled).count(), 1);
    assert!(keys.iter().any(|key| key.name == "renamed key"));

    let _ = std::fs::remove_file(path);
}

#[tokio::test]
async fn non_admin_api_key_creation_stays_owned_by_caller() {
    let (db, path) = test_db().await;
    let db = Arc::new(db);
    let caller = db
        .create_user("caller".into(), None, false, None)
        .await
        .unwrap();
    let other = db
        .create_user("other".into(), None, false, None)
        .await
        .unwrap();
    let caller_id = caller.id.expect("caller id");
    let other_id = other.id.expect("other id");

    let (status, _) = crate::handlers::auth::create_api_key::<SqliteDb>(
        Extension(db.clone()),
        Extension(AuthContext {
            principal_id: Some(caller_id),
            is_admin: false,
            kind: PrincipalKind::User,
        }),
        Json(CreateApiKeyRequest {
            name: "attempted service key".into(),
            user_id: Some(other_id),
            is_service: true,
            expires_at: None,
        }),
    )
    .await;
    assert_eq!(status, StatusCode::OK);

    let caller_keys = db.list_api_keys(Some(caller_id)).await.unwrap();
    assert_eq!(caller_keys.len(), 1);
    assert!(!caller_keys[0].is_service);
    assert_eq!(caller_keys[0].user_id, Some(caller_id));
    assert!(db.list_api_keys(Some(other_id)).await.unwrap().is_empty());

    let _ = std::fs::remove_file(path);
}

#[tokio::test]
async fn bootstrap_database_persists_explicit_jwt_secret() {
    let (db, path) = test_db().await;

    let db = Arc::new(db);
    bootstrap_database(
        &db,
        &BootstrapOptions {
            auth_jwt_secret: Some("explicit-secret".into()),
            auth_jwt_secret_previous: None,
            auth_bootstrap_admin: None,
            auth_bootstrap_admin_force: false,
            auth_bootstrap_service_api_key: None,
            auth_bootstrap_service_api_key_name: None,
        },
    )
    .await
    .unwrap();

    assert_eq!(
        load_jwt_secret(db.as_ref()).await.unwrap(),
        b"explicit-secret".to_vec()
    );

    let _ = std::fs::remove_file(path);
}

#[tokio::test]
async fn bootstrap_database_generates_jwt_secret_once() {
    let (db, path) = test_db().await;

    let db = Arc::new(db);
    bootstrap_database(&db, &BootstrapOptions::default())
        .await
        .unwrap();
    let first = load_jwt_secret(db.as_ref()).await.unwrap();

    bootstrap_database(&db, &BootstrapOptions::default())
        .await
        .unwrap();
    let second = load_jwt_secret(db.as_ref()).await.unwrap();

    assert!(!first.is_empty());
    assert_eq!(first, second);

    let _ = std::fs::remove_file(path);
}

#[tokio::test]
async fn visible_workflow_ids_include_direct_and_team_grants() {
    let (db, path) = test_db().await;
    let direct = crate::repository::upsert_workflow(&db, &workflow(None, "direct"))
        .await
        .unwrap();
    let team = crate::repository::upsert_workflow(&db, &workflow(None, "team"))
        .await
        .unwrap();
    let user = db
        .create_user("member".into(), None, false, None)
        .await
        .unwrap();
    let user_id = user.id.expect("user id");
    let team_record = db.create_team("ops".into()).await.unwrap();
    let team_id = team_record.id.expect("team id");
    db.add_team_member(team_id, user_id).await.unwrap();
    db.create_grant(Grant {
        id: None,
        resource_type: ResourceType::Workflow,
        resource_id: direct.id.expect("workflow id"),
        principal_type: PrincipalType::User,
        principal_id: user_id,
        permission: Permission::View,
        created_at: chrono::Utc::now(),
    })
    .await
    .unwrap();
    db.create_grant(Grant {
        id: None,
        resource_type: ResourceType::Workflow,
        resource_id: team.id.expect("workflow id"),
        principal_type: PrincipalType::Team,
        principal_id: team_id,
        permission: Permission::Run,
        created_at: chrono::Utc::now(),
    })
    .await
    .unwrap();

    let visible = crate::authz::visible_workflow_ids(
        &db,
        &AuthContext {
            principal_id: Some(user_id),
            is_admin: false,
            kind: PrincipalKind::User,
        },
    )
    .await
    .expect("scoped set");

    assert_eq!(visible.len(), 2);
    assert!(visible.contains(&direct.id.unwrap()));
    assert!(visible.contains(&team.id.unwrap()));

    let _ = std::fs::remove_file(path);
}

#[tokio::test]
async fn workflow_permission_takes_highest_of_user_and_team_grants() {
    let (db, path) = test_db().await;
    let wf = crate::repository::upsert_workflow(&db, &workflow(None, "shared"))
        .await
        .unwrap();
    let workflow_id = wf.id.expect("workflow id");
    let user_id = db
        .create_user("member".into(), None, false, None)
        .await
        .unwrap()
        .id
        .expect("user id");
    let team_id = db
        .create_team("ops".into())
        .await
        .unwrap()
        .id
        .expect("team id");
    db.add_team_member(team_id, user_id).await.unwrap();
    // a weak direct grant and a stronger team grant on the same workflow.
    db.create_grant(grant(
        workflow_id,
        PrincipalType::User,
        user_id,
        Permission::View,
    ))
    .await
    .unwrap();
    db.create_grant(grant(
        workflow_id,
        PrincipalType::Team,
        team_id,
        Permission::Edit,
    ))
    .await
    .unwrap();

    let ctx = user_ctx(user_id);
    let effective = crate::authz::workflow_permission(&db, &ctx, workflow_id).await;
    assert_eq!(effective, Some(Permission::Edit));

    // edit (and everything below it) is allowed; own is not.
    assert!(
        crate::authz::require_workflow(&db, &ctx, workflow_id, Permission::Run)
            .await
            .is_ok()
    );
    assert!(
        crate::authz::require_workflow(&db, &ctx, workflow_id, Permission::Edit)
            .await
            .is_ok()
    );
    let denied = crate::authz::require_workflow(&db, &ctx, workflow_id, Permission::Own).await;
    assert_eq!(
        denied.expect_err("own should be denied").0,
        StatusCode::FORBIDDEN
    );

    let _ = std::fs::remove_file(path);
}

#[tokio::test]
async fn workflow_permission_is_none_without_a_grant() {
    let (db, path) = test_db().await;
    let wf = crate::repository::upsert_workflow(&db, &workflow(None, "private"))
        .await
        .unwrap();
    let workflow_id = wf.id.expect("workflow id");
    let user_id = db
        .create_user("stranger".into(), None, false, None)
        .await
        .unwrap()
        .id
        .expect("user id");

    let ctx = user_ctx(user_id);
    assert_eq!(
        crate::authz::workflow_permission(&db, &ctx, workflow_id).await,
        None
    );
    let denied = crate::authz::require_workflow(&db, &ctx, workflow_id, Permission::View).await;
    assert_eq!(
        denied.expect_err("view should be denied").0,
        StatusCode::FORBIDDEN
    );

    let _ = std::fs::remove_file(path);
}

#[tokio::test]
async fn workflow_permission_admin_owns_everything_without_grants() {
    let (db, path) = test_db().await;
    let wf = crate::repository::upsert_workflow(&db, &workflow(None, "any"))
        .await
        .unwrap();
    let workflow_id = wf.id.expect("workflow id");

    let admin = AuthContext {
        principal_id: None,
        is_admin: true,
        kind: PrincipalKind::User,
    };
    assert_eq!(
        crate::authz::workflow_permission(&db, &admin, workflow_id).await,
        Some(Permission::Own)
    );
    assert!(
        crate::authz::require_workflow(&db, &admin, workflow_id, Permission::Own)
            .await
            .is_ok()
    );

    let _ = std::fs::remove_file(path);
}

#[tokio::test]
async fn wdl_evaluate_accepts_legacy_lowered_expression() {
    let request = crate::handlers::wdl::EvaluateExpressionRequest {
        expression: Some(json!({ "$concat": ["hello ", { "$ref": { "params": ["name"] } }] })),
        source: None,
        kind: WdlFragmentKind::Expression,
        context: json!({ "input": { "name": "Ada" } }),
    };

    let Json(value) = crate::handlers::wdl::evaluate_expression(Json(request))
        .await
        .expect("evaluate");

    assert_eq!(value, Value::from("hello Ada"));
}

#[tokio::test]
async fn wdl_evaluate_accepts_source_fragments() {
    let request = crate::handlers::wdl::EvaluateExpressionRequest {
        expression: None,
        source: Some("params.count >= 3 && exists params.count".into()),
        kind: WdlFragmentKind::Condition,
        context: json!({ "input": { "count": 3 } }),
    };

    let Json(value) = crate::handlers::wdl::evaluate_expression(Json(request))
        .await
        .expect("evaluate");

    assert_eq!(value, Value::from(true));
}

#[tokio::test]
async fn wdl_analyze_validates_source_fragments() {
    let (db, path) = test_db().await;
    let Json(diagnostics) = crate::handlers::wdl::analyze_wdl(
        Extension(Arc::new(db)),
        Json(crate::handlers::wdl::WdlSourceRequest {
            source: "params.count >".into(),
            fragment: Some(WdlFragmentKind::Condition),
        }),
    )
    .await;

    assert_eq!(diagnostics.len(), 1);
    assert_eq!(diagnostics[0].severity, "error");
    let _ = std::fs::remove_file(path);
}

#[tokio::test]
async fn workflow_runs_can_be_named_and_fetched_by_open_name() {
    let (db, path) = test_db().await;
    let workflow = crate::repository::upsert_workflow(&db, &workflow(None, "Ticket Work"))
        .await
        .unwrap();
    let workflow_id = workflow.id.unwrap();
    let open = crate::repository::create_workflow_run(
        &db,
        workflow_id,
        json!({}),
        false,
        Some("Ticket Work: ITP-123".into()),
        Default::default(),
    )
    .await
    .unwrap();
    let terminal = crate::repository::create_workflow_run(
        &db,
        workflow_id,
        json!({}),
        false,
        Some("Ticket Work: ITP-123".into()),
        Default::default(),
    )
    .await
    .unwrap();
    crate::repository::update_workflow_run_status(
        &db,
        terminal.id,
        WorkflowStatus::Succeeded,
        None,
        None,
        None,
    )
    .await
    .unwrap();

    let open_only =
        crate::repository::fetch_workflow_runs_by_name(&db, "Ticket Work: ITP-123".into(), true)
            .await
            .unwrap();

    assert_eq!(open.name.as_deref(), Some("Ticket Work: ITP-123"));
    assert_eq!(
        open_only.iter().map(|run| run.id).collect::<Vec<_>>(),
        vec![open.id]
    );
    let _ = std::fs::remove_file(path);
}

#[tokio::test]
async fn ready_node_processing_reduces_start_to_action_dispatch() {
    let (db, path) = test_db().await;
    let mut workflow = workflow(None, "ready-reducer");
    workflow.definition = WorkflowGraph::from_value(json!({
        "start": "start",
        "nodes": [
            { "id": "start", "kind": "start", "transitions": { "next": { "$node": "run" } } },
            {
                "id": "run",
                "kind": "action",
                "action": {
                    "provider": "test",
                    "function": "execute",
                    "configuration": { "message": "hello" }
                },
                "transitions": { "on_success": { "$node": "done" } }
            },
            { "id": "done", "kind": "end" }
        ]
    }))
    .unwrap();
    let workflow = db.upsert_workflow(&workflow).await.unwrap();
    let run = crate::repository::create_workflow_run(
        &db,
        workflow.id.unwrap(),
        json!({}),
        false,
        None,
        Default::default(),
    )
    .await
    .unwrap();
    let ready = crate::repository::claim_ready_nodes(
        &db,
        "scheduler-a".into(),
        chrono::Utc::now() + chrono::Duration::seconds(30),
        10,
    )
    .await
    .unwrap();
    assert_eq!(ready.len(), 1);

    crate::repository::complete_ready_node(&db, ready[0].id, "scheduler-a".into(), None)
        .await
        .unwrap();

    let (updated, nodes) = crate::repository::fetch_workflow_run(&db, run.id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(updated.status, WorkflowStatus::Running);
    assert_eq!(updated.active_node_id.as_deref(), Some("run"));
    assert!(
        nodes
            .iter()
            .any(|node| node.node_id == "run" && node.status == WorkflowStatus::Running)
    );
    let dispatches = db.fetch_pending_action_dispatches(10).await.unwrap();
    assert_eq!(dispatches.len(), 1);
    assert_eq!(dispatches[0].command.node_id, "run");

    let _ = std::fs::remove_file(path);
}

#[tokio::test]
async fn output_nodes_write_automation_events_for_the_events_tab() {
    let (db, path) = test_db().await;
    let mut workflow = workflow(None, "output-events");
    workflow.definition = WorkflowGraph::from_value(json!({
        "start": "start",
        "nodes": [
            { "id": "start", "kind": "start", "transitions": { "next": { "$node": "output" } } },
            {
                "id": "output",
                "kind": "output",
                "parameters": {
                    "event_type": "workflow.routed",
                    "data": { "ok": true, "count": 1 }
                },
                "transitions": { "next": { "$node": "done" } }
            },
            { "id": "done", "kind": "end" }
        ]
    }))
    .unwrap();
    let workflow = db.upsert_workflow(&workflow).await.unwrap();
    let run = crate::repository::create_workflow_run(
        &db,
        workflow.id.unwrap(),
        json!({}),
        false,
        None,
        Default::default(),
    )
    .await
    .unwrap();
    let ready = crate::repository::claim_ready_nodes(
        &db,
        "scheduler-a".into(),
        chrono::Utc::now() + chrono::Duration::seconds(30),
        10,
    )
    .await
    .unwrap();
    assert_eq!(ready.len(), 1);

    crate::repository::complete_ready_node(&db, ready[0].id, "scheduler-a".into(), None)
        .await
        .unwrap();

    let events = db
        .fetch_automation_records("automation_events".into(), Some(run.id), None)
        .await
        .unwrap();
    assert_eq!(events.len(), 1);
    let event = &events[0];
    let metadata = event
        .get("metadata")
        .and_then(Value::as_object)
        .cloned()
        .unwrap_or_default();
    assert_eq!(
        event.get("event_type").and_then(Value::as_str),
        Some("workflow.routed")
    );
    assert_eq!(
        event.get("provider").and_then(Value::as_str),
        Some("runinator")
    );
    assert_eq!(
        event.get("status").and_then(Value::as_str),
        Some("output_recorded")
    );
    assert_eq!(
        metadata
            .get("data")
            .and_then(Value::as_object)
            .and_then(|data| data.get("ok"))
            .and_then(Value::as_bool),
        Some(true)
    );

    let _ = std::fs::remove_file(path);
}

#[tokio::test]
async fn pure_compute_node_reruns_in_loop_body() {
    let (db, path) = test_db().await;
    let mut workflow = workflow(None, "loop-compute");
    workflow.definition = WorkflowGraph::from_value(json!({
        "start": "start",
        "nodes": [
            { "id": "start", "kind": "start", "transitions": { "next": { "$node": "each" } } },
            {
                "id": "each",
                "kind": "loop",
                "parameters": { "items": { "$ref": { "input": ["xs"] } } },
                "max_iterations": 10,
                "transitions": {
                    "next": { "$node": "double" },
                    "on_success": { "$node": "done" }
                }
            },
            {
                "id": "double",
                "kind": "action",
                "action": {
                    "provider": "std",
                    "function": "run",
                    "configuration": {
                        "program": [
                            { "$return": { "$mul": [{ "$ref": { "node": "each", "output": ["item"] } }, 2] } }
                        ]
                    }
                },
                "transitions": { "on_success": { "$node": "each" } }
            },
            { "id": "done", "kind": "end" }
        ]
    }))
    .unwrap();
    let workflow = db.upsert_workflow(&workflow).await.unwrap();
    let run = crate::repository::create_workflow_run(
        &db,
        workflow.id.unwrap(),
        json!({ "xs": [1, 2, 3] }),
        false,
        None,
        Default::default(),
    )
    .await
    .unwrap();

    drain_ready_nodes(&db).await;

    let (updated, nodes) = crate::repository::fetch_workflow_run(&db, run.id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(updated.status, WorkflowStatus::Succeeded);
    // the compute body ran once per item, re-creating a fresh node run each iteration.
    let runs = nodes
        .iter()
        .filter(|node| node.node_id == "double" && node.status == WorkflowStatus::Succeeded)
        .count();
    assert_eq!(runs, 3, "compute body should run once per loop item");
    // and never dispatched to a worker.
    assert!(
        db.fetch_pending_action_dispatches(10)
            .await
            .unwrap()
            .is_empty()
    );

    let _ = std::fs::remove_file(path);
}

#[tokio::test]
async fn pure_compute_node_reduces_in_process_without_dispatch() {
    let (db, path) = test_db().await;
    let mut workflow = workflow(None, "pure-compute");
    workflow.definition = WorkflowGraph::from_value(json!({
        "start": "start",
        "nodes": [
            { "id": "start", "kind": "start", "transitions": { "next": { "$node": "calc" } } },
            {
                "id": "calc",
                "kind": "action",
                "action": {
                    "provider": "std",
                    "function": "run",
                    "configuration": {
                        "program": [
                            { "$let": "total", "value": { "$add": [{ "$ref": { "input": ["a"] } }, 3] } },
                            { "$return": { "total": { "$ref": { "let": ["total"] } } } }
                        ]
                    }
                },
                "transitions": { "on_success": { "$node": "done" } }
            },
            { "id": "done", "kind": "end" }
        ]
    }))
    .unwrap();
    let workflow = db.upsert_workflow(&workflow).await.unwrap();
    let run = crate::repository::create_workflow_run(
        &db,
        workflow.id.unwrap(),
        json!({ "a": 4 }),
        false,
        None,
        Default::default(),
    )
    .await
    .unwrap();

    drain_ready_nodes(&db).await;

    let (updated, nodes) = crate::repository::fetch_workflow_run(&db, run.id)
        .await
        .unwrap()
        .unwrap();
    // the pure compute node reduced in-process and the run reached the end node.
    assert_eq!(updated.status, WorkflowStatus::Succeeded);
    let calc = nodes.iter().find(|node| node.node_id == "calc").unwrap();
    assert_eq!(calc.status, WorkflowStatus::Succeeded);
    assert_eq!(calc.output_json, Some(json!({ "total": 7 })));
    // no worker dispatch was enqueued for the pure node.
    assert!(
        db.fetch_pending_action_dispatches(10)
            .await
            .unwrap()
            .is_empty()
    );

    let _ = std::fs::remove_file(path);
}

#[tokio::test]
async fn compute_goto_sets_active_node() {
    let (db, path) = test_db().await;
    let mut workflow = workflow(None, "compute-goto");
    workflow.definition = WorkflowGraph::from_value(json!({
        "start": "start",
        "nodes": [
            { "id": "start", "kind": "start", "transitions": { "next": { "$node": "gate" } } },
            {
                "id": "gate",
                "kind": "action",
                "action": {
                    "provider": "std",
                    "function": "run",
                    "configuration": {
                        "program": [
                            { "$if": { "value": { "$ref": { "input": ["x"] } }, "less_than": 0 },
                              "then": [ { "$goto": "fail" } ],
                              "else": [] },
                            { "$return": "ok" }
                        ]
                    }
                },
                "transitions": { "on_success": { "$node": "done" } }
            },
            { "id": "fail", "kind": "fail" },
            { "id": "done", "kind": "end" }
        ]
    }))
    .unwrap();
    let workflow = db.upsert_workflow(&workflow).await.unwrap();
    let run = crate::repository::create_workflow_run(
        &db,
        workflow.id.unwrap(),
        json!({ "x": -1 }),
        false,
        None,
        Default::default(),
    )
    .await
    .unwrap();

    drain_ready_nodes(&db).await;

    let (updated, _) = crate::repository::fetch_workflow_run(&db, run.id)
        .await
        .unwrap()
        .unwrap();
    // goto fail routed the run to the fail node, ending the run as failed.
    assert_eq!(updated.status, WorkflowStatus::Failed);

    let _ = std::fs::remove_file(path);
}

#[tokio::test]
async fn action_failure_schedules_retry_with_backoff() {
    let (db, path) = test_db().await;
    let mut workflow = workflow(None, "action-retry");
    workflow.definition = WorkflowGraph::from_value(json!({
        "start": "start",
        "nodes": [
            { "id": "start", "kind": "start", "transitions": { "next": { "$node": "run" } } },
            {
                "id": "run",
                "kind": "action",
                "action": {
                    "provider": "test",
                    "function": "execute",
                    "configuration": { "message": "hello" }
                },
                "retry": { "max_attempts": 3 },
                "transitions": { "on_failure": { "$node": "failed" } }
            },
            { "id": "failed", "kind": "fail" },
            { "id": "end", "kind": "end" }
        ]
    }))
    .unwrap();
    let workflow = db.upsert_workflow(&workflow).await.unwrap();
    let run = crate::repository::create_workflow_run(
        &db,
        workflow.id.unwrap(),
        json!({}),
        false,
        None,
        Default::default(),
    )
    .await
    .unwrap();

    drain_ready_nodes(&db).await;
    let dispatch = db.fetch_pending_action_dispatches(10).await.unwrap()[0].clone();
    db.mark_action_dispatch_published(dispatch.id)
        .await
        .unwrap();
    let event = WorkflowResultEvent::status(&dispatch.command, WorkflowStatus::Failed, None, None);
    crate::repository::apply_workflow_result_event(&db, &event)
        .await
        .unwrap();

    drain_ready_nodes(&db).await;

    let (updated, nodes) = crate::repository::fetch_workflow_run(&db, run.id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(updated.status, WorkflowStatus::Waiting);
    assert_eq!(updated.active_node_id.as_deref(), Some("run"));
    let run_node = nodes.iter().find(|node| node.node_id == "run").unwrap();
    assert_eq!(run_node.status, WorkflowStatus::Queued);
    assert_eq!(run_node.attempt, 1);
    let retry_ready = db
        .fetch_pending_ready_nodes(chrono::Utc::now(), 10)
        .await
        .unwrap()
        .into_iter()
        .find(|ready| ready.workflow_run_id == run.id && ready.node_id == "run")
        .expect("retry ready node is pending");
    assert!(retry_ready.ready_at > chrono::Utc::now());
    assert!(
        db.fetch_pending_action_dispatches(10)
            .await
            .unwrap()
            .is_empty()
    );

    let _ = std::fs::remove_file(path);
}

#[tokio::test]
async fn action_retry_republishes_dispatch_after_backoff() {
    let (db, path) = test_db().await;
    let run_id = seed_run(
        &db,
        "action-retry-redispatch",
        json!({
            "start": "start",
            "nodes": [
                { "id": "start", "kind": "start", "transitions": { "next": { "$node": "run" } } },
                {
                    "id": "run",
                    "kind": "action",
                    "action": { "provider": "test", "function": "execute", "configuration": {} },
                    "retry": { "max_attempts": 3 },
                    "transitions": { "on_failure": { "$node": "failed" } }
                },
                { "id": "failed", "kind": "fail" },
                { "id": "end", "kind": "end" }
            ]
        }),
    )
    .await;

    drain_ready_nodes(&db).await;
    let dispatch = db.fetch_pending_action_dispatches(10).await.unwrap()[0].clone();
    db.mark_action_dispatch_published(dispatch.id)
        .await
        .unwrap();
    let event = WorkflowResultEvent::status(&dispatch.command, WorkflowStatus::Failed, None, None);
    crate::repository::apply_workflow_result_event(&db, &event)
        .await
        .unwrap();
    drain_ready_nodes(&db).await;

    // wait out the first retry backoff, then drive the retry ready node to its re-dispatch.
    tokio::time::sleep(Duration::from_millis(1500)).await;
    drain_ready_nodes(&db).await;

    // the retried attempt must publish a fresh outbox row; reusing the first attempt's dedupe key
    // would collide with the already-published row and park the run in `running` forever.
    let pending = db.fetch_pending_action_dispatches(10).await.unwrap();
    assert_eq!(pending.len(), 1, "retry must enqueue a fresh dispatch");
    assert_eq!(pending[0].command.attempt, 2);
    assert_ne!(pending[0].dedupe_key, dispatch.dedupe_key);
    let (run, _) = crate::repository::fetch_workflow_run(&db, run_id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(run.status, WorkflowStatus::Running);

    let _ = std::fs::remove_file(path);
}

#[tokio::test]
async fn duplicate_terminal_result_event_still_enqueues_drive() {
    let (db, path) = test_db().await;
    let run_id = seed_run(
        &db,
        "duplicate-result-drive",
        json!({
            "start": "start",
            "nodes": [
                { "id": "start", "kind": "start", "transitions": { "next": { "$node": "run" } } },
                {
                    "id": "run",
                    "kind": "action",
                    "action": { "provider": "test", "function": "execute", "configuration": {} },
                    "transitions": { "next": { "$node": "end" } }
                },
                { "id": "end", "kind": "end" }
            ]
        }),
    )
    .await;

    drain_ready_nodes(&db).await;
    let dispatch = db.fetch_pending_action_dispatches(10).await.unwrap()[0].clone();
    db.mark_action_dispatch_published(dispatch.id)
        .await
        .unwrap();
    let event = WorkflowResultEvent::status(
        &dispatch.command,
        WorkflowStatus::Succeeded,
        Some(json!({ "ok": true })),
        None,
    );
    assert!(
        crate::repository::apply_workflow_result_event(&db, &event)
            .await
            .unwrap()
    );
    drain_ready_nodes(&db).await;
    let (run, _) = crate::repository::fetch_workflow_run(&db, run_id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(run.status, WorkflowStatus::Succeeded);

    // a redelivered duplicate can follow a crash that lost the first drive enqueue; it must still
    // enqueue a drive even though the event itself is not re-applied.
    assert!(
        !crate::repository::apply_workflow_result_event(&db, &event)
            .await
            .unwrap()
    );
    let pending = db
        .fetch_pending_ready_nodes(chrono::Utc::now(), 10)
        .await
        .unwrap();
    assert!(
        pending
            .iter()
            .any(|node| node.workflow_run_id == run_id && node.node_id == "run")
    );

    let _ = std::fs::remove_file(path);
}

#[tokio::test]
async fn action_node_timeout_recovers_parked_run() {
    let (db, path) = test_db().await;
    let run_id = seed_run(
        &db,
        "action-timeout",
        json!({
            "start": "start",
            "nodes": [
                { "id": "start", "kind": "start", "transitions": { "next": { "$node": "run" } } },
                {
                    "id": "run",
                    "kind": "action",
                    "timeout_seconds": 1,
                    "action": { "provider": "test", "function": "execute", "configuration": {} },
                    "transitions": { "next": { "$node": "end" } }
                },
                { "id": "end", "kind": "end" }
            ]
        }),
    )
    .await;

    drain_ready_nodes(&db).await;
    let dispatch = db.fetch_pending_action_dispatches(10).await.unwrap()[0].clone();
    db.mark_action_dispatch_published(dispatch.id)
        .await
        .unwrap();

    // no worker result ever arrives; the armed timeout wake must settle the parked node.
    tokio::time::sleep(Duration::from_millis(1500)).await;
    drain_ready_nodes(&db).await;

    let (run, nodes) = crate::repository::fetch_workflow_run(&db, run_id)
        .await
        .unwrap()
        .unwrap();
    let node_run = nodes.iter().find(|node| node.node_id == "run").unwrap();
    assert_eq!(node_run.status, WorkflowStatus::TimedOut);
    assert_eq!(run.status, WorkflowStatus::TimedOut);

    let _ = std::fs::remove_file(path);
}

#[test]
fn merges_json_objects() {
    let defaults = json!({ "a": 1, "b": 2 });
    let parameters = json!({ "b": 3, "c": 4 });
    let merged = crate::repository::merge_json_object(&defaults, &parameters);
    assert_eq!(merged, json!({ "a": 1, "b": 3, "c": 4 }));
}

#[test]
fn registered_provider_items_become_provider_metadata() {
    let providers = crate::provider_metadata_from_items(vec![json!({
        "document": {
            "name": "github",
            "actions": [
                { "function_name": "create_pr", "description": "Create a pull request" }
            ]
        }
    })])
    .expect("provider metadata parses");

    assert_eq!(providers[0].name, "github");
    assert_eq!(providers[0].actions[0].function_name, "create_pr");
}

#[test]
fn provider_metadata_becomes_registered_catalog_item() {
    let item = crate::provider_catalog_item(&runinator_models::providers::ProviderMetadata {
        name: "git".into(),
        actions: vec![runinator_models::providers::ActionMetadata::new(
            "diff", "Get diff",
        )],
        metadata: Default::default(),
    });

    assert_eq!(item["item_type"], "provider_metadata");
    assert_eq!(item["document"]["name"], "git");
    assert_eq!(item["document"]["actions"][0]["function_name"], "diff");
}

#[test]
fn validate_workflow_returns_normalized_definition() {
    let workflow = workflow(None, "validate");
    let validated = crate::repository::validate_workflow_definition(&workflow).unwrap();

    assert_eq!(validated.name, "validate");
    assert_eq!(validated.definition.start.as_deref(), Some("start"));
}

#[test]
fn validate_workflow_rejects_invalid_definition_without_persistence() {
    let mut workflow = workflow(None, "invalid");
    workflow.definition = WorkflowGraph::from_value(json!({
        "start": "start",
        "nodes": [
            { "id": "start", "kind": "start", "transitions": { "next": { "$node": "missing" } } },
            { "id": "done", "kind": "end" }
        ]
    }))
    .unwrap();

    assert!(crate::repository::validate_workflow_definition(&workflow).is_err());
}

#[test]
fn validation_error_response_exposes_structured_type_diagnostic() {
    let err = WorkflowValidationError::TypeDiagnostic(WorkflowTypeDiagnostic {
        path: "action parameter 'config.name'".into(),
        expected: "string".into(),
        actual: "integer".into(),
        message: "action parameter 'config.name' expected string, got integer".into(),
    });

    let (status, axum::Json(response)) = crate::responses::validation_error(&err);
    assert_eq!(status, axum::http::StatusCode::BAD_REQUEST);
    match response {
        crate::models::ApiResponse::ApiError(error) => {
            assert_eq!(
                error.path.as_deref(),
                Some("action parameter 'config.name'")
            );
            assert_eq!(error.expected.as_deref(), Some("string"));
            assert_eq!(error.actual.as_deref(), Some("integer"));
        }
        _ => panic!("expected api error"),
    }
}

#[tokio::test]
async fn export_all_includes_workflows_and_matching_triggers() {
    let (db, path) = test_db().await;
    let saved = crate::repository::upsert_workflow(&db, &workflow(None, "export-all"))
        .await
        .unwrap();
    let workflow_id = saved.id.unwrap();
    crate::repository::upsert_workflow_trigger(&db, &trigger(None, workflow_id))
        .await
        .unwrap();

    let bundle = crate::repository::export_workflow_bundle(&db, None)
        .await
        .unwrap();

    assert_eq!(bundle.workflows.len(), 1);
    assert_eq!(bundle.workflows[0].id, Some(workflow_id));
    assert_eq!(bundle.triggers.len(), 1);
    assert_eq!(bundle.triggers[0].workflow_id, workflow_id);
    let _ = std::fs::remove_file(path);
}

#[tokio::test]
async fn export_one_includes_only_that_workflow_and_its_triggers() {
    let (db, path) = test_db().await;
    let first = crate::repository::upsert_workflow(&db, &workflow(None, "first"))
        .await
        .unwrap();
    let second = crate::repository::upsert_workflow(&db, &workflow(None, "second"))
        .await
        .unwrap();
    let first_id = first.id.unwrap();
    let second_id = second.id.unwrap();
    crate::repository::upsert_workflow_trigger(&db, &trigger(None, first_id))
        .await
        .unwrap();
    crate::repository::upsert_workflow_trigger(&db, &trigger(None, second_id))
        .await
        .unwrap();

    let bundle = crate::repository::export_workflow_bundle(&db, Some(second_id))
        .await
        .unwrap();

    assert_eq!(bundle.workflows.len(), 1);
    assert_eq!(bundle.workflows[0].id, Some(second_id));
    assert_eq!(bundle.triggers.len(), 1);
    assert_eq!(bundle.triggers[0].workflow_id, second_id);
    let _ = std::fs::remove_file(path);
}

#[tokio::test]
async fn import_upserts_workflows_before_triggers() {
    let (db, path) = test_db().await;
    let wf_id = Uuid::now_v7();
    let trig_id = Uuid::now_v7();
    let bundle = WorkflowBundle {
        workflows: vec![workflow(Some(wf_id), "imported")],
        triggers: vec![trigger(Some(trig_id), wf_id)],
    };

    let saved = crate::repository::import_workflow_bundle(&db, bundle)
        .await
        .unwrap();

    assert_eq!(saved.workflows[0].id, Some(wf_id));
    assert_eq!(saved.triggers[0].id, Some(trig_id));
    assert!(db.fetch_workflow(wf_id).await.unwrap().is_some());
    assert_eq!(db.fetch_workflow_triggers(wf_id).await.unwrap().len(), 1);
    let _ = std::fs::remove_file(path);
}

#[tokio::test]
async fn import_skips_workflow_when_name_already_exists() {
    let (db, path) = test_db().await;
    let first = WorkflowBundle {
        workflows: vec![workflow(None, "Core Team SDLC Pipeline")],
        triggers: vec![],
    };
    let initial = crate::repository::import_workflow_bundle(&db, first)
        .await
        .unwrap();
    let initial_version = initial.workflows[0].version;
    let initial_definition = initial.workflows[0].definition.clone();
    let mut changed = workflow(None, "Core Team SDLC Pipeline");
    changed.version = runinator_models::semver::SemVer::new(2, 0, 0);
    changed.definition = WorkflowGraph::from_value(json!({
        "start": "done",
        "nodes": [
            { "id": "done", "kind": "end" }
        ]
    }))
    .unwrap();
    let second = WorkflowBundle {
        workflows: vec![changed.clone()],
        triggers: vec![],
    };

    let saved = crate::repository::import_workflow_bundle(&db, second)
        .await
        .unwrap();
    let workflows = db.fetch_workflows().await.unwrap();

    // re-importing the same workflow name leaves the existing row untouched.
    assert_eq!(workflows.len(), 1);
    assert_eq!(saved.workflows[0].id, workflows[0].id);
    assert_eq!(workflows[0].name, "Core Team SDLC Pipeline");
    assert_eq!(workflows[0].version, initial_version);
    assert_eq!(workflows[0].definition, initial_definition);
    let _ = std::fs::remove_file(path);
}

#[tokio::test]
async fn import_overwrite_updates_existing_workflow_in_place() {
    let (db, path) = test_db().await;
    let first = WorkflowBundle {
        workflows: vec![workflow(None, "Core Team SDLC Pipeline")],
        triggers: vec![],
    };
    let initial = crate::repository::import_workflow_bundle(&db, first)
        .await
        .unwrap();
    let existing_id = initial.workflows[0].id;
    assert_ne!(
        initial.workflows[0].version,
        runinator_models::semver::SemVer::new(2, 0, 0)
    );

    // an explicit re-apply carries no id and no newer timestamp, but overwrite must still win.
    let mut changed = workflow(None, "Core Team SDLC Pipeline");
    changed.version = runinator_models::semver::SemVer::new(2, 0, 0);
    changed.definition = WorkflowGraph::from_value(json!({
        "start": "done",
        "nodes": [
            { "id": "done", "kind": "end" }
        ]
    }))
    .unwrap();
    let second = WorkflowBundle {
        workflows: vec![changed.clone()],
        triggers: vec![],
    };

    let saved = crate::repository::import_workflow_bundle_with(&db, second, true)
        .await
        .unwrap();
    let workflows = db.fetch_workflows().await.unwrap();

    // the existing row is updated in place: same id, bumped version, no duplicate row. the skip
    // path would have left the stored version unchanged, so version == 2 proves the overwrite.
    assert_eq!(workflows.len(), 1);
    assert_eq!(saved.workflows[0].id, existing_id);
    assert_eq!(workflows[0].id, existing_id);
    assert_eq!(
        workflows[0].version,
        runinator_models::semver::SemVer::new(2, 0, 0)
    );
    let _ = std::fs::remove_file(path);
}

#[tokio::test]
async fn import_upserts_existing_workflow_when_id_is_present() {
    let (db, path) = test_db().await;
    let first = WorkflowBundle {
        workflows: vec![workflow(None, "Core Team SDLC Pipeline")],
        triggers: vec![],
    };
    let initial = crate::repository::import_workflow_bundle(&db, first)
        .await
        .unwrap();
    let existing_id = initial.workflows[0].id;

    // a save from the command center carries the existing id and must overwrite.
    let mut changed = initial.workflows[0].clone();
    changed.version = runinator_models::semver::SemVer::new(2, 0, 0);
    changed.definition = WorkflowGraph::from_value(json!({
        "start": "done",
        "nodes": [
            { "id": "done", "kind": "end" }
        ]
    }))
    .unwrap();
    let second = WorkflowBundle {
        workflows: vec![changed.clone()],
        triggers: vec![],
    };

    let saved = crate::repository::import_workflow_bundle(&db, second)
        .await
        .unwrap();
    let workflows = db.fetch_workflows().await.unwrap();

    assert_eq!(workflows.len(), 1);
    assert_eq!(saved.workflows[0].id, existing_id);
    // an upsert bumps the version to 2; a skip would have left it at 1.
    assert_eq!(
        workflows[0].version,
        runinator_models::semver::SemVer::new(2, 0, 0)
    );
    assert_eq!(
        saved.workflows[0].version,
        runinator_models::semver::SemVer::new(2, 0, 0)
    );
    let _ = std::fs::remove_file(path);
}

#[tokio::test]
async fn import_overwrites_id_less_workflow_when_incoming_is_newer() {
    let (db, path) = test_db().await;
    let first = WorkflowBundle {
        workflows: vec![workflow(None, "pack")],
        triggers: vec![],
    };
    crate::repository::import_workflow_bundle(&db, first)
        .await
        .unwrap();

    // a pack import carrying a future updated_at is newer than the stored copy.
    let mut newer = workflow(None, "pack");
    newer.version = runinator_models::semver::SemVer::new(5, 0, 0);
    newer.updated_at = chrono::DateTime::from_timestamp(4_102_444_800, 0);
    let saved = crate::repository::import_workflow_bundle(
        &db,
        WorkflowBundle {
            workflows: vec![newer],
            triggers: vec![],
        },
    )
    .await
    .unwrap();
    let workflows = db.fetch_workflows().await.unwrap();

    assert_eq!(workflows.len(), 1);
    assert_eq!(
        workflows[0].version,
        runinator_models::semver::SemVer::new(5, 0, 0)
    );
    assert_eq!(
        saved.workflows[0].version,
        runinator_models::semver::SemVer::new(5, 0, 0)
    );
    let _ = std::fs::remove_file(path);
}

#[tokio::test]
async fn import_skips_id_less_workflow_when_incoming_is_older() {
    let (db, path) = test_db().await;
    let first = WorkflowBundle {
        workflows: vec![workflow(None, "pack")],
        triggers: vec![],
    };
    let initial = crate::repository::import_workflow_bundle(&db, first)
        .await
        .unwrap();
    let initial_version = initial.workflows[0].version;

    // a pack import carrying a past updated_at is older than the stored copy.
    let mut older = workflow(None, "pack");
    older.version = runinator_models::semver::SemVer::new(5, 0, 0);
    older.updated_at = chrono::DateTime::from_timestamp(1, 0);
    crate::repository::import_workflow_bundle(
        &db,
        WorkflowBundle {
            workflows: vec![older],
            triggers: vec![],
        },
    )
    .await
    .unwrap();
    let workflows = db.fetch_workflows().await.unwrap();

    assert_eq!(workflows.len(), 1);
    assert_eq!(workflows[0].version, initial_version);
    let _ = std::fs::remove_file(path);
}

#[tokio::test]
async fn duplicate_workflow_creates_bumped_sibling() {
    let (db, path) = test_db().await;
    let initial = crate::repository::import_workflow_bundle(
        &db,
        WorkflowBundle {
            workflows: vec![workflow(None, "Core Team SDLC Pipeline")],
            triggers: vec![],
        },
    )
    .await
    .unwrap();
    let original = initial.workflows[0].clone();
    let original_id = original.id.unwrap();

    let copy = crate::repository::duplicate_workflow(
        &db,
        original_id,
        runinator_models::semver::SemVerBump::Minor,
    )
    .await
    .unwrap();
    let workflows = db.fetch_workflows().await.unwrap();

    // a new disabled row sharing the name, with the minor version bumped.
    assert_eq!(workflows.len(), 2);
    assert_ne!(copy.id, original.id);
    assert_eq!(copy.name, original.name);
    assert!(!copy.enabled);
    assert_eq!(
        copy.version,
        original
            .version
            .bump(runinator_models::semver::SemVerBump::Minor)
    );
    let _ = std::fs::remove_file(path);
}

#[tokio::test]
async fn result_consumer_acks_duplicate_deliveries_and_persists_results_once() {
    let (db, path) = test_db().await;
    let db = Arc::new(db);
    let node_run = create_node_run(&db).await;
    let command = action_command(node_run.workflow_run_id, node_run.id, &node_run.node_id);
    let chunk = WorkflowResultEvent::chunk(
        &command,
        NewRunChunk {
            stream: "log".into(),
            content: "hello".into(),
        },
    );
    let status = WorkflowResultEvent::status(
        &command,
        WorkflowStatus::Succeeded,
        Some(json!({ "ok": true })),
        Some("done".into()),
    );
    let artifact = WorkflowResultEvent::artifact(
        &command,
        NewRunArtifact {
            name: "report.json".into(),
            mime_type: "application/json".into(),
            size_bytes: 17,
            uri: "memory://report.json".into(),
            metadata: json!({ "source": "test" }),
        },
    );
    let broker = Arc::new(RecordingBroker::new());
    let broker_for_consumer: Arc<dyn Broker> = broker.clone();
    let (events, _rx) = tokio::sync::broadcast::channel(16);
    let bus = crate::events::EventBus::new(events, broker_for_consumer.clone());
    let shutdown = Arc::new(Notify::new());
    let consumer = tokio::spawn(crate::result_consumer::run_result_consumer(
        db.clone(),
        broker_for_consumer,
        bus,
        shutdown.clone(),
    ));

    publish_duplicate_results(&broker, &[chunk.clone(), status.clone(), artifact.clone()]).await;
    wait_until(|| broker.result_acks().len() == 6).await;

    shutdown.notify_waiters();
    tokio::time::timeout(Duration::from_secs(1), consumer)
        .await
        .unwrap()
        .unwrap();

    let chunks = db
        .fetch_workflow_node_run_chunks(node_run.id, None, 100)
        .await
        .unwrap();
    assert_eq!(chunks.len(), 1);
    assert_eq!(chunks[0].stream, "log");
    assert_eq!(chunks[0].content, "hello");

    let node_run = db
        .fetch_workflow_node_run(node_run.id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(node_run.status, WorkflowStatus::Succeeded);
    assert_eq!(node_run.output_json, Some(json!({ "ok": true })));
    assert_eq!(node_run.message.as_deref(), Some("done"));

    let artifacts = db
        .fetch_workflow_node_run_artifacts(node_run.id)
        .await
        .unwrap();
    assert_eq!(artifacts.len(), 1);
    assert_eq!(artifacts[0].name, "report.json");
    assert_eq!(artifacts[0].uri, "memory://report.json");

    let received = broker.result_receives();
    let acked = broker.result_acks();
    assert_eq!(received.len(), 6);
    assert_eq!(acked.len(), 6);
    assert_eq!(received, acked);
    assert!(broker.result_nacks().is_empty());
    let _ = std::fs::remove_file(path);
}

#[tokio::test]
async fn result_consumer_dead_letters_poison_result_events_after_retries() {
    let (db, path) = test_db().await;
    let db = Arc::new(db);
    let node_run = create_node_run(&db).await;
    let command = action_command(
        node_run.workflow_run_id,
        node_run.id,
        "__force_result_persist_failure__",
    );
    let poison = WorkflowResultEvent::chunk(
        &command,
        NewRunChunk {
            stream: "log".into(),
            content: "poison".into(),
        },
    );
    let broker = Arc::new(RecordingBroker::new());
    let broker_for_consumer: Arc<dyn Broker> = broker.clone();
    let (events, _rx) = tokio::sync::broadcast::channel(16);
    let bus = crate::events::EventBus::new(events, broker_for_consumer.clone());
    let shutdown = Arc::new(Notify::new());
    let consumer = tokio::spawn(crate::result_consumer::run_result_consumer_with_policy(
        db.clone(),
        broker_for_consumer,
        bus,
        shutdown.clone(),
        crate::result_consumer::ResultConsumerPolicy::new(2, Duration::from_millis(1)),
    ));

    broker
        .publish_result(ResultMessage {
            event: poison,
            dedupe_key: Some("poison-result".into()),
            enqueued_at: chrono::Utc::now(),
        })
        .await
        .unwrap();
    wait_until(|| broker.result_acks().len() == 1 && broker.result_nacks().len() == 1).await;

    shutdown.notify_waiters();
    tokio::time::timeout(Duration::from_secs(1), consumer)
        .await
        .unwrap()
        .unwrap();

    let chunks = db
        .fetch_workflow_node_run_chunks(node_run.id, None, 100)
        .await
        .unwrap();
    assert!(chunks.is_empty());

    // the poison event leaves a durable dead-letter record on the result channel.
    let dead_letters = db.fetch_dead_letters(None, 100).await.unwrap();
    assert_eq!(dead_letters.len(), 1);
    assert_eq!(
        dead_letters[0]
            .get("channel")
            .and_then(runinator_models::value::Value::as_str),
        Some("result")
    );

    let _ = std::fs::remove_file(path);
}

#[test]
fn result_consumer_backoff_grows_and_is_capped() {
    let policy = crate::result_consumer::ResultConsumerPolicy::new(5, Duration::from_millis(100));
    // full jitter keeps every delay within [0, base * 2^(attempt-1)] for early attempts.
    for attempt in 1..=4u32 {
        let ceiling = 100u128 * (1u128 << (attempt - 1));
        let backoff = policy.backoff_for(attempt).as_millis();
        assert!(
            backoff <= ceiling,
            "attempt {attempt} backoff {backoff} exceeded ceiling {ceiling}"
        );
    }
    // a large attempt count is clamped to the 30s max backoff and cannot overflow.
    let huge = policy.backoff_for(40).as_millis();
    assert!(huge <= 30_000, "backoff {huge} exceeded max_backoff");
}

#[tokio::test]
async fn deliverables_promote_node_artifacts_to_the_run() {
    let (db, path) = test_db().await;
    let node_run = create_node_run(&db).await;
    // a node artifact, then a deliverable promoting it to the run.
    let artifact = db
        .add_workflow_node_run_artifact(
            node_run.id,
            &NewRunArtifact {
                name: "dump.csv".into(),
                mime_type: "text/csv".into(),
                size_bytes: 42,
                uri: "memory://dump.csv".into(),
                metadata: json!({}),
            },
        )
        .await
        .unwrap();

    // the run-wide artifact fetch (used to build runtime context) sees it.
    let run_artifacts = db
        .fetch_workflow_node_run_artifacts_for_run(node_run.workflow_run_id)
        .await
        .unwrap();
    assert_eq!(run_artifacts.len(), 1);
    assert_eq!(run_artifacts[0].id, artifact.id);

    let stored = db
        .add_workflow_run_deliverable(&NewWorkflowRunDeliverable {
            workflow_run_id: node_run.workflow_run_id,
            node_id: node_run.node_id.clone(),
            artifact_id: artifact.id,
            name: "report".into(),
            mime_type: artifact.mime_type.clone(),
            size_bytes: artifact.size_bytes,
            uri: artifact.uri.clone(),
            metadata: json!({}),
        })
        .await
        .unwrap();
    assert_eq!(stored.name, "report");

    let deliverables = db
        .fetch_workflow_run_deliverables(node_run.workflow_run_id)
        .await
        .unwrap();
    assert_eq!(deliverables.len(), 1);
    assert_eq!(deliverables[0].artifact_id, artifact.id);
    assert_eq!(deliverables[0].name, "report");
    assert_eq!(deliverables[0].uri, "memory://dump.csv");
    let _ = std::fs::remove_file(path);
}

async fn test_db() -> (SqliteDb, std::path::PathBuf) {
    let path = std::env::temp_dir().join(format!(
        "runinator-ws-workflows-{}.db",
        uuid::Uuid::new_v4()
    ));
    let db = SqliteDb::new(path.to_str().unwrap()).await.unwrap();
    db.run_init_scripts(&Vec::new()).await.unwrap();
    (db, path)
}

async fn create_node_run(db: &SqliteDb) -> WorkflowNodeRun {
    let workflow = crate::repository::upsert_workflow(db, &workflow(None, "result-consumer"))
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
    crate::repository::create_workflow_node_run(db, run.id, "node-a".into(), json!({}))
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
        },
        attempt: 1,
        parameters: json!({}),
        trace_id: Uuid::nil(),
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

#[test]
fn ancestors_in_snapshot_returns_topological_path() {
    let snapshot = WorkflowDefinition {
        id: Some(Uuid::now_v7()),
        name: "ancestors".into(),
        namespace: None,
        version: runinator_models::semver::SemVer::new(1, 0, 0),
        enabled: true,
        input_type: runinator_models::types::RuninatorType::Any,
        definition: WorkflowGraph::from_value(json!({
            "start": "start",
            "nodes": [
                { "id": "start", "kind": "start", "transitions": { "next": { "$node": "a" } } },
                { "id": "a", "kind": "action", "action": { "provider": "console", "function": "run" }, "transitions": { "next": { "$node": "b" } } },
                { "id": "b", "kind": "action", "action": { "provider": "console", "function": "run" }, "transitions": { "next": { "$node": "c" } } },
                { "id": "c", "kind": "action", "action": { "provider": "console", "function": "run" }, "transitions": { "next": { "$node": "end" } } },
                { "id": "end", "kind": "end" }
            ]
        }))
        .unwrap(),
        created_at: None,
        updated_at: None,
    };
    let ancestors = crate::repository::ancestors_in_snapshot(&snapshot, "c").unwrap();
    assert!(ancestors.contains(&"start".to_string()));
    assert!(ancestors.contains(&"a".to_string()));
    assert!(ancestors.contains(&"b".to_string()));
    assert!(!ancestors.contains(&"c".to_string()));
    // start must come before a, a before b.
    let pos_start = ancestors.iter().position(|n| n == "start").unwrap();
    let pos_a = ancestors.iter().position(|n| n == "a").unwrap();
    let pos_b = ancestors.iter().position(|n| n == "b").unwrap();
    assert!(pos_start < pos_a);
    assert!(pos_a < pos_b);
}

#[test]
fn ancestors_in_snapshot_refuses_control_flow_ancestor() {
    let snapshot = WorkflowDefinition {
        id: Some(Uuid::now_v7()),
        name: "loop_ancestor".into(),
        namespace: None,
        version: runinator_models::semver::SemVer::new(1, 0, 0),
        enabled: true,
        input_type: runinator_models::types::RuninatorType::Any,
        definition: WorkflowGraph::from_value(json!({
            "start": "start",
            "nodes": [
                { "id": "start", "kind": "start", "transitions": { "next": { "$node": "loop1" } } },
                { "id": "loop1", "kind": "loop", "parameters": { "items": [], "target": { "$node": "inside" } }, "transitions": { "next": { "$node": "end" } } },
                { "id": "inside", "kind": "action", "action": { "provider": "console", "function": "run" }, "transitions": { "next": { "$node": "loop1" } } },
                { "id": "end", "kind": "end" }
            ]
        }))
        .unwrap(),
        created_at: None,
        updated_at: None,
    };
    let result = crate::repository::ancestors_in_snapshot(&snapshot, "inside");
    assert!(
        result.is_err(),
        "expected refusal for control-flow ancestor"
    );
    let message = result.unwrap_err().to_string();
    assert!(
        message.contains("control-flow") || message.contains("Loop") || message.contains("safely"),
        "error should mention control flow: {message}"
    );
}

#[test]
fn ancestors_in_snapshot_rejects_missing_step() {
    let snapshot = WorkflowDefinition {
        id: Some(Uuid::now_v7()),
        name: "missing".into(),
        namespace: None,
        version: runinator_models::semver::SemVer::new(1, 0, 0),
        enabled: true,
        input_type: runinator_models::types::RuninatorType::Any,
        definition: WorkflowGraph::from_value(json!({
            "start": "start",
            "nodes": [
                { "id": "start", "kind": "start", "transitions": { "next": { "$node": "end" } } },
                { "id": "end", "kind": "end" }
            ]
        }))
        .unwrap(),
        created_at: None,
        updated_at: None,
    };
    let result = crate::repository::ancestors_in_snapshot(&snapshot, "nope");
    assert!(result.is_err());
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

#[tokio::test]
async fn validate_workflow_rejects_invalid_subflow_id() {
    let (db, path) = test_db().await;

    // create a valid target workflow
    let target = crate::repository::upsert_workflow(&db, &workflow(None, "target-workflow"))
        .await
        .unwrap();
    let target_id = target.id.unwrap();

    // create a workflow with a subflow that references a non-existent workflow
    let mut main_workflow = workflow(None, "main-with-invalid-subflow");
    main_workflow.definition = WorkflowGraph::from_value(json!({
        "start": "start",
        "nodes": [
            { "id": "start", "kind": "start", "transitions": { "next": { "$node": "subflow-node" } } },
            {
                "id": "subflow-node",
                "kind": "subflow",
                "subflow_id": Uuid::now_v7().to_string(),  // non-existent workflow id
                "transitions": { "next": { "$node": "end" } }
            },
            { "id": "end", "kind": "end" }
        ]
    }))
    .unwrap();

    // validation should fail because the subflow references a non-existent workflow
    let result =
        crate::repository::validate_workflow_definition_with_catalog(&db, &main_workflow).await;
    assert!(result.is_err());

    // now test with a valid subflow id
    let mut valid_workflow = workflow(None, "main-with-valid-subflow");
    valid_workflow.definition = WorkflowGraph::from_value(json!({
        "start": "start",
        "nodes": [
            { "id": "start", "kind": "start", "transitions": { "next": { "$node": "subflow-node" } } },
            {
                "id": "subflow-node",
                "kind": "subflow",
                "subflow_id": target_id,
                "transitions": { "next": { "$node": "end" } }
            },
            { "id": "end", "kind": "end" }
        ]
    }))
    .unwrap();

    // validation should succeed because the subflow references a valid workflow
    let result =
        crate::repository::validate_workflow_definition_with_catalog(&db, &valid_workflow).await;
    assert!(result.is_ok());

    let _ = std::fs::remove_file(path);
}

// --- rich control-flow reducer coverage --------------------------------------

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

#[tokio::test]
async fn reducer_runs_loop_node_over_all_items() {
    let (db, path) = test_db().await;
    let run_id = seed_run(
        &db,
        "loop-flow",
        json!({
            "start": "start",
            "nodes": [
                { "id": "start", "kind": "start", "transitions": { "next": { "$node": "loop" } } },
                {
                    "id": "loop",
                    "kind": "loop",
                    "parameters": { "items": ["a", "b", "c"] },
                    "transitions": { "next": { "$node": "body" }, "on_success": { "$node": "done" } }
                },
                { "id": "body", "kind": "output", "transitions": { "on_success": { "$node": "loop" } } },
                { "id": "done", "kind": "end" }
            ]
        }),
    )
    .await;

    let run = run_to_completion(&db, run_id).await;
    assert_eq!(run.status, WorkflowStatus::Succeeded);

    let nodes = db.fetch_workflow_node_runs(run_id).await.unwrap();
    let loop_succeeded = nodes
        .iter()
        .filter(|n| n.node_id == "loop" && n.status == WorkflowStatus::Succeeded)
        .count();
    let body_succeeded = nodes
        .iter()
        .filter(|n| n.node_id == "body" && n.status == WorkflowStatus::Succeeded)
        .count();
    // three iterations plus the exhausted visit that exits the loop; the body runs once per item.
    assert_eq!(loop_succeeded, 4);
    assert_eq!(body_succeeded, 3);
    let _ = std::fs::remove_file(path);
}

#[tokio::test]
async fn reducer_dispatches_loop_body_action_once_per_item() {
    let (db, path) = test_db().await;
    let run_id = seed_run(
        &db,
        "loop-action-flow",
        json!({
            "start": "start",
            "nodes": [
                { "id": "start", "kind": "start", "transitions": { "next": { "$node": "loop" } } },
                {
                    "id": "loop",
                    "kind": "loop",
                    "parameters": { "items": ["a", "b", "c", "d"] },
                    "transitions": { "next": { "$node": "body" }, "on_success": { "$node": "done" } }
                },
                {
                    "id": "body",
                    "kind": "action",
                    "action": { "provider": "console", "function": "run" },
                    "transitions": { "on_success": { "$node": "loop" } }
                },
                { "id": "done", "kind": "end" }
            ]
        }),
    )
    .await;

    let run = run_to_completion(&db, run_id).await;
    assert_eq!(run.status, WorkflowStatus::Succeeded);

    let nodes = db.fetch_workflow_node_runs(run_id).await.unwrap();
    // a re-entered loop body must dispatch a fresh action run per iteration, not reuse the first.
    let body_succeeded = nodes
        .iter()
        .filter(|n| n.node_id == "body" && n.status == WorkflowStatus::Succeeded)
        .count();
    assert_eq!(body_succeeded, 4);
    let _ = std::fs::remove_file(path);
}

#[tokio::test]
async fn reducer_fans_out_parallel_branches_and_joins() {
    let (db, path) = test_db().await;
    let run_id = seed_run(
        &db,
        "parallel-flow",
        json!({
            "start": "start",
            "nodes": [
                { "id": "start", "kind": "start", "transitions": { "next": { "$node": "fork" } } },
                {
                    "id": "fork",
                    "kind": "parallel",
                    "parameters": { "branches": [{ "$node": "a" }, { "$node": "b" }] },
                    "transitions": {}
                },
                {
                    "id": "a",
                    "kind": "action",
                    "action": { "provider": "test", "function": "execute" },
                    "transitions": { "on_success": { "$node": "join" } }
                },
                {
                    "id": "b",
                    "kind": "action",
                    "action": { "provider": "test", "function": "execute" },
                    "transitions": { "on_success": { "$node": "join" } }
                },
                {
                    "id": "join",
                    "kind": "join",
                    "parameters": { "wait_for": [{ "$node": "a" }, { "$node": "b" }], "mode": "all" },
                    "transitions": { "on_success": { "$node": "done" } }
                },
                { "id": "done", "kind": "end" }
            ]
        }),
    )
    .await;

    let run = run_to_completion(&db, run_id).await;
    assert_eq!(run.status, WorkflowStatus::Succeeded);

    let nodes = db.fetch_workflow_node_runs(run_id).await.unwrap();
    for branch in ["a", "b", "join"] {
        assert!(
            nodes
                .iter()
                .any(|n| n.node_id == branch && n.status == WorkflowStatus::Succeeded),
            "branch {branch} should have succeeded"
        );
    }
    let _ = std::fs::remove_file(path);
}

#[tokio::test]
async fn reducer_maps_items_through_target_node() {
    let (db, path) = test_db().await;
    let run_id = seed_run(
        &db,
        "map-flow",
        json!({
            "start": "start",
            "nodes": [
                { "id": "start", "kind": "start", "transitions": { "next": { "$node": "map" } } },
                {
                    "id": "map",
                    "kind": "map",
                    "parameters": { "items": [1, 2], "target": { "$node": "each" } },
                    "transitions": { "on_success": { "$node": "done" } }
                },
                { "id": "each", "kind": "output", "transitions": { "on_success": { "$node": "map" } } },
                { "id": "done", "kind": "end" }
            ]
        }),
    )
    .await;

    let run = run_to_completion(&db, run_id).await;
    assert_eq!(run.status, WorkflowStatus::Succeeded);
    // each item runs the body in its own child run; the map gathers their outputs in order.
    let outputs = map_node_outputs(&db, run_id).await;
    assert_eq!(outputs.len(), 2);
    let _ = std::fs::remove_file(path);
}

/// fetch the ordered per-item outputs recorded on a run's `map` node.
async fn map_node_outputs(db: &SqliteDb, run_id: Uuid) -> Vec<Value> {
    let nodes = db.fetch_workflow_node_runs(run_id).await.unwrap();
    nodes
        .iter()
        .filter(|n| n.node_id == "map" && n.status == WorkflowStatus::Succeeded)
        .find_map(|n| n.output_json.as_ref())
        .and_then(|output| output.get("outputs"))
        .and_then(|outputs| outputs.as_array().cloned())
        .unwrap_or_default()
}

#[tokio::test]
async fn reducer_maps_items_concurrently_in_order() {
    let (db, path) = test_db().await;
    let run_id = seed_run(
        &db,
        "map-concurrent-flow",
        json!({
            "start": "start",
            "nodes": [
                { "id": "start", "kind": "start", "transitions": { "next": { "$node": "map" } } },
                {
                    "id": "map",
                    "kind": "map",
                    "parameters": {
                        "items": [10, 20, 30, 40, 50],
                        "target": { "$node": "each" },
                        "concurrency": 3
                    },
                    "transitions": { "on_success": { "$node": "done" } }
                },
                {
                    "id": "each",
                    "kind": "output",
                    "parameters": { "data": { "$ref": { "node": "map", "output": ["item"] } } },
                    "transitions": { "on_success": { "$node": "map" } }
                },
                { "id": "done", "kind": "end" }
            ]
        }),
    )
    .await;

    let run = run_to_completion(&db, run_id).await;
    assert_eq!(run.status, WorkflowStatus::Succeeded);
    // five items fanned out three-at-a-time still gather in item order.
    let outputs = map_node_outputs(&db, run_id).await;
    let items: Vec<i64> = outputs
        .iter()
        .filter_map(|output| output.get("data").and_then(Value::as_i64))
        .collect();
    assert_eq!(items, vec![10, 20, 30, 40, 50]);
    let _ = std::fs::remove_file(path);
}

#[tokio::test]
async fn reducer_map_fails_fast_when_item_fails() {
    let (db, path) = test_db().await;
    let run_id = seed_run(
        &db,
        "map-fail-flow",
        json!({
            "start": "start",
            "nodes": [
                { "id": "start", "kind": "start", "transitions": { "next": { "$node": "map" } } },
                {
                    "id": "map",
                    "kind": "map",
                    "parameters": {
                        "items": [1, 2, 3],
                        "target": { "$node": "work" },
                        "concurrency": 3
                    },
                    "transitions": { "on_success": { "$node": "done" } }
                },
                {
                    "id": "work",
                    "kind": "action",
                    "action": { "provider": "console", "function": "run" },
                    "transitions": { "on_success": { "$node": "map" } }
                },
                { "id": "done", "kind": "end" }
            ]
        }),
    )
    .await;

    // drive the fan-out, then fail the first item's action and succeed the rest.
    let mut failed_one = false;
    let mut run = crate::repository::fetch_workflow_run(&db, run_id)
        .await
        .unwrap()
        .unwrap()
        .0;
    for _ in 0..64 {
        drain_ready_nodes(&db).await;
        run = crate::repository::fetch_workflow_run(&db, run_id)
            .await
            .unwrap()
            .unwrap()
            .0;
        if run.status.is_terminal() {
            break;
        }
        let dispatches = db.fetch_pending_action_dispatches(50).await.unwrap();
        if dispatches.is_empty() {
            break;
        }
        for dispatch in dispatches {
            db.mark_action_dispatch_published(dispatch.id)
                .await
                .unwrap();
            let status = if failed_one {
                WorkflowStatus::Succeeded
            } else {
                failed_one = true;
                WorkflowStatus::Failed
            };
            let event = WorkflowResultEvent::status(&dispatch.command, status, None, None);
            crate::repository::apply_workflow_result_event(&db, &event)
                .await
                .unwrap();
        }
    }
    // a single failed item fails the whole map (no on_failure routing here).
    assert_eq!(run.status, WorkflowStatus::Failed);
    let _ = std::fs::remove_file(path);
}

#[tokio::test]
async fn reducer_try_node_runs_body_then_finally() {
    let (db, path) = test_db().await;
    let run_id = seed_run(
        &db,
        "try-flow",
        json!({
            "start": "start",
            "nodes": [
                { "id": "start", "kind": "start", "transitions": { "next": { "$node": "try" } } },
                {
                    "id": "try",
                    "kind": "try",
                    "parameters": { "body": { "$node": "body" }, "finally": { "$node": "cleanup" } },
                    "transitions": { "on_success": { "$node": "done" } }
                },
                { "id": "body", "kind": "output", "transitions": { "on_success": { "$node": "try" } } },
                { "id": "cleanup", "kind": "output", "transitions": { "on_success": { "$node": "try" } } },
                { "id": "done", "kind": "end" }
            ]
        }),
    )
    .await;

    let run = run_to_completion(&db, run_id).await;
    assert_eq!(run.status, WorkflowStatus::Succeeded);
    let nodes = db.fetch_workflow_node_runs(run_id).await.unwrap();
    for stage in ["body", "cleanup"] {
        assert!(
            nodes
                .iter()
                .any(|n| n.node_id == stage && n.status == WorkflowStatus::Succeeded),
            "{stage} should have run"
        );
    }
    let _ = std::fs::remove_file(path);
}

#[tokio::test]
async fn reducer_parks_approval_then_resolution_wakes_and_completes() {
    let (db, path) = test_db().await;
    let run_id = seed_run(
        &db,
        "approval-flow",
        json!({
            "start": "start",
            "nodes": [
                { "id": "start", "kind": "start", "transitions": { "next": { "$node": "gate" } } },
                {
                    "id": "gate",
                    "kind": "approval",
                    "parameters": { "prompt": "approve?" },
                    "transitions": { "on_success": { "$node": "done" } }
                },
                { "id": "done", "kind": "end" }
            ]
        }),
    )
    .await;

    // the approval node parks the run waiting for an external decision.
    let run = run_to_completion(&db, run_id).await;
    assert_eq!(run.status, WorkflowStatus::ApprovalRequired);
    assert_eq!(run.active_node_id.as_deref(), Some("gate"));

    // resolve the approval the way the api handler would.
    let approvals = db
        .fetch_automation_records("approval_requests".into(), Some(run_id), None)
        .await
        .unwrap();
    assert_eq!(approvals.len(), 1);
    let approval_id = approvals[0]
        .get("id")
        .and_then(Value::as_str)
        .and_then(|raw| raw.parse::<Uuid>().ok())
        .unwrap();
    crate::repository::resolve_approval(&db, approval_id, true, None, None, None)
        .await
        .unwrap();

    // resolution should have enqueued a ready node; draining now completes the run.
    let run = run_to_completion(&db, run_id).await;
    assert_eq!(run.status, WorkflowStatus::Succeeded);
    let _ = std::fs::remove_file(path);
}

#[tokio::test]
async fn reducer_subflow_waits_for_child_and_child_terminal_wakes_parent() {
    let (db, path) = test_db().await;

    // child workflow that completes on its own.
    let mut child = workflow(None, "child-flow");
    child.definition = WorkflowGraph::from_value(json!({
        "start": "start",
        "nodes": [
            { "id": "start", "kind": "start", "transitions": { "next": { "$node": "done" } } },
            { "id": "done", "kind": "end" }
        ]
    }))
    .unwrap();
    let child = db.upsert_workflow(&child).await.unwrap();
    let child_id = child.id.unwrap();

    // parent that launches the child as a waiting subflow.
    let mut parent = workflow(None, "parent-flow");
    parent.definition = WorkflowGraph::from_value(json!({
        "start": "start",
        "nodes": [
            { "id": "start", "kind": "start", "transitions": { "next": { "$node": "sub" } } },
            {
                "id": "sub",
                "kind": "subflow",
                "subflow_id": child_id,
                "subflow": { "type": "wait" },
                "transitions": { "on_success": { "$node": "done" } }
            },
            { "id": "done", "kind": "end" }
        ]
    }))
    .unwrap();
    let parent = db.upsert_workflow(&parent).await.unwrap();
    let parent_run = crate::repository::create_workflow_run(
        &db,
        parent.id.unwrap(),
        json!({}),
        false,
        None,
        Default::default(),
    )
    .await
    .unwrap();

    // draining drives the parent to launch + the child to completion; the terminal child wakes the
    // parent's subflow node, which then transitions to its end.
    drain_ready_nodes(&db).await;
    let (run, _) = crate::repository::fetch_workflow_run(&db, parent_run.id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(
        run.status,
        WorkflowStatus::Succeeded,
        "parent run should complete after child finishes, got {:?}",
        run.status
    );
    let _ = std::fs::remove_file(path);
}
