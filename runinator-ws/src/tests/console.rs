//! covers the console's central split: a pure cell settles in the request, an effectful one becomes
//! a run.
//!
//! the direction that matters is the second. a cell wrongly treated as pure would execute a provider
//! action inside an http handler — no run to record it, no retry, no timeout, no cancellation — so
//! there is a test that an action call leaves a `workflow_run_id` behind rather than a value.

use super::*;

use std::sync::Arc;

use runinator_models::console::{ConsoleCellKind, ConsoleCellStatus, NewConsoleCell};

use crate::models::ApiResponse;

fn admin() -> AuthContext {
    AuthContext {
        principal_id: Some(Uuid::new_v4()),
        is_admin: true,
        kind: PrincipalKind::User,
        org_id: None,
        org_role: None,
    }
}

fn events() -> crate::events::EventSender {
    let (tx, _rx) = tokio::sync::broadcast::channel(64);
    crate::events::EventBus::new(tx, Arc::new(InMemoryBroker::new()))
}

// the web service seeds built-in provider metadata at startup; a bare test database has none, and a
// scratch workflow calling `console.run` would fail validation for want of it. seeded here so the
// test sees what production does.
async fn seed_providers(db: &Arc<SqliteDb>) {
    for provider in runinator_provider_catalog::metadata() {
        let item = crate::repository::provider_catalog_item(&provider);
        crate::repository::upsert_catalog_item(db.as_ref(), item)
            .await
            .unwrap();
    }
}

async fn session(db: &Arc<SqliteDb>) -> Uuid {
    let (status, body) = crate::handlers::console::create_console_session::<SqliteDb>(
        Extension(db.clone()),
        Extension(admin()),
        Json(serde_json::from_value(json!({ "name": "scratch" }).into()).unwrap()),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    match body.0 {
        ApiResponse::ConsoleSession(session) => session.id,
        _ => panic!("unexpected response"),
    }
}

async fn cell(db: &Arc<SqliteDb>, session_id: Uuid, source: &str, label: Option<&str>) -> Uuid {
    let (status, body) = crate::handlers::console::create_console_cell::<SqliteDb>(
        Extension(db.clone()),
        Extension(admin()),
        Path(session_id),
        Json(NewConsoleCell {
            source: source.to_string(),
            label: label.map(str::to_string),
            position: None,
        }),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    match body.0 {
        ApiResponse::ConsoleCell(cell) => cell.id,
        _ => panic!("unexpected response"),
    }
}

async fn run(db: &Arc<SqliteDb>, cell_id: Uuid) -> runinator_models::console::ConsoleCell {
    let (status, body) = crate::handlers::console::run_console_cell::<SqliteDb>(
        Extension(db.clone()),
        Extension(events()),
        Extension(admin()),
        Path(cell_id),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    match body.0 {
        ApiResponse::ConsoleCell(cell) => cell,
        _ => panic!("unexpected response"),
    }
}

#[tokio::test]
async fn a_pure_cell_settles_in_the_same_request() {
    let (db, db_path) = test_db().await;
    let db = Arc::new(db);
    let session_id = session(&db).await;

    let settled = run(&db, cell(&db, session_id, "1 + 2", Some("sum")).await).await;

    // no run, no polling: an arithmetic cell that took a second and left a row in the run history
    // would make the console useless as a scratchpad.
    assert_eq!(settled.status, ConsoleCellStatus::Succeeded);
    assert_eq!(settled.kind, Some(ConsoleCellKind::Expression));
    assert_eq!(settled.result, Some(json!(3)));
    assert!(settled.workflow_run_id.is_none());

    let _ = std::fs::remove_file(db_path);
}

#[tokio::test]
async fn a_later_cell_reads_an_earlier_cells_result() {
    let (db, db_path) = test_db().await;
    let db = Arc::new(db);
    let session_id = session(&db).await;

    run(&db, cell(&db, session_id, "20", Some("base")).await).await;
    let settled = run(&db, cell(&db, session_id, "params.base + 1", None).await).await;

    // the shared scope is what makes this a notebook rather than a list of unrelated snippets.
    assert_eq!(
        settled.result,
        Some(json!(21)),
        "status {:?} error {:?}",
        settled.status,
        settled.error
    );

    let _ = std::fs::remove_file(db_path);
}

#[tokio::test]
async fn an_effectful_cell_becomes_a_run_rather_than_being_evaluated_here() {
    let (db, db_path) = test_db().await;
    let db = Arc::new(db);
    seed_providers(&db).await;
    let session_id = session(&db).await;

    let settled = run(
        &db,
        cell(&db, session_id, "console.run(command: \"echo hi\")", None).await,
    )
    .await;

    // the load-bearing assertion: a provider action must never be executed inside the handler.
    assert_eq!(settled.kind, Some(ConsoleCellKind::Workflow));
    assert_eq!(
        settled.status,
        ConsoleCellStatus::Running,
        "error {:?}",
        settled.error
    );
    assert!(settled.result.is_none());
    let run_id = settled.workflow_run_id.expect("a scratch run");

    // and the scratch workflow it created stays out of the authored workflow list.
    let listed = crate::repository::fetch_workflows(db.as_ref())
        .await
        .unwrap();
    assert!(
        listed.is_empty(),
        "scratch workflows must not be listed, got {:?}",
        listed.iter().map(|w| w.name.clone()).collect::<Vec<_>>()
    );
    let all = crate::repository::fetch_workflows_with_managed(db.as_ref(), true)
        .await
        .unwrap();
    assert_eq!(all.len(), 1);
    assert!(all[0].name.starts_with("console."));

    // the run carries console provenance, so it is attributable without probing json.
    let (run, _) = crate::repository::fetch_workflow_run(db.as_ref(), run_id)
        .await
        .unwrap()
        .expect("run");
    assert_eq!(
        run.trigger_source_kind,
        Some(runinator_models::replicas::TriggerSourceKind::Console)
    );

    let _ = std::fs::remove_file(db_path);
}

#[tokio::test]
async fn a_failing_cell_does_not_leave_a_stale_binding() {
    let (db, db_path) = test_db().await;
    let db = Arc::new(db);
    let session_id = session(&db).await;

    let cell_id = cell(&db, session_id, "1 + 1", Some("value")).await;
    assert_eq!(run(&db, cell_id).await.result, Some(json!(2)));

    // edit it into something that cannot compile — a typo'd action name, the realistic case — then
    // re-run. note a *missing path* would not do: `params.nope.missing` resolves to null rather
    // than failing, which is the expression language's optional-chaining behaviour and not an error.
    let _ = crate::handlers::console::update_console_cell::<SqliteDb>(
        Extension(db.clone()),
        Extension(admin()),
        Path(cell_id),
        Json(NewConsoleCell {
            source: "console.notarealaction(command: \"x\")".into(),
            label: Some("value".into()),
            position: None,
        }),
    )
    .await;
    let settled = run(&db, cell_id).await;
    assert_eq!(settled.status, ConsoleCellStatus::Failed);
    assert!(settled.error.is_some());

    // the old value must not survive: a later cell reading `params.value` while the cell that
    // produced it is visibly red would be reading a stale answer.
    let bindings = crate::repository::console::fetch_bindings(db.as_ref(), session_id)
        .await
        .unwrap();
    assert!(
        bindings.iter().all(|binding| binding.name != "value"),
        "a failed cell must drop its binding, got {bindings:?}"
    );

    let _ = std::fs::remove_file(db_path);
}

#[tokio::test]
async fn editing_a_cell_clears_the_result_it_no_longer_produced() {
    let (db, db_path) = test_db().await;
    let db = Arc::new(db);
    let session_id = session(&db).await;

    let cell_id = cell(&db, session_id, "1 + 1", None).await;
    run(&db, cell_id).await;

    let (status, body) = crate::handlers::console::update_console_cell::<SqliteDb>(
        Extension(db.clone()),
        Extension(admin()),
        Path(cell_id),
        Json(NewConsoleCell {
            source: "2 + 2".into(),
            label: None,
            position: None,
        }),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    match body.0 {
        ApiResponse::ConsoleCell(cell) => {
            // a result shown beside changed source is a stale answer presented as a current one.
            assert_eq!(cell.status, ConsoleCellStatus::Idle);
            assert!(cell.result.is_none());
            assert!(cell.kind.is_none());
        }
        _ => panic!("unexpected response"),
    }

    let _ = std::fs::remove_file(db_path);
}

#[tokio::test]
async fn deleting_a_session_takes_its_cells_and_scope() {
    let (db, db_path) = test_db().await;
    let db = Arc::new(db);
    let session_id = session(&db).await;
    run(&db, cell(&db, session_id, "1", Some("one")).await).await;

    let (status, _) = crate::handlers::console::delete_console_session::<SqliteDb>(
        Extension(db.clone()),
        Extension(admin()),
        Path(session_id),
    )
    .await;
    assert_eq!(status, StatusCode::OK);

    assert!(
        crate::repository::console::fetch_session_detail(db.as_ref(), session_id)
            .await
            .unwrap()
            .is_none()
    );
    assert!(
        crate::repository::console::fetch_bindings(db.as_ref(), session_id)
            .await
            .unwrap()
            .is_empty()
    );

    let _ = std::fs::remove_file(db_path);
}

#[tokio::test]
async fn the_console_is_gated_on_a_capability() {
    use runinator_models::capabilities::Capability;
    use runinator_ws_middleware::authz::AuthContextExt;

    // a console cell can start a workflow run, so using the console is a privilege rather than a
    // view. gated on a named capability so the backend and the ui reference one dictionary.
    let member = AuthContext {
        principal_id: Some(Uuid::new_v4()),
        is_admin: false,
        kind: PrincipalKind::User,
        org_id: None,
        org_role: None,
    };
    assert!(member.require_capability(Capability::ConsoleUse).is_err());
    assert!(admin().require_capability(Capability::ConsoleUse).is_ok());

    let (db, db_path) = test_db().await;
    let db = Arc::new(db);
    let (status, _) = crate::handlers::console::get_console_sessions::<SqliteDb>(
        Extension(db.clone()),
        Extension(member),
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN);

    let _ = std::fs::remove_file(db_path);
}

#[tokio::test]
async fn a_cell_endpoint_is_gated_even_though_it_takes_no_session_id() {
    use runinator_models::capabilities::Capability;

    // every non-listing endpoint reaches the database through `require_session`, and the gate lives
    // there too — so an endpoint that forgot its own check is still stopped.
    let (db, db_path) = test_db().await;
    let db = Arc::new(db);
    let session_id = session(&db).await;
    let cell_id = cell(&db, session_id, "1 + 1", None).await;

    let member = AuthContext {
        principal_id: Some(Uuid::new_v4()),
        is_admin: false,
        kind: PrincipalKind::User,
        org_id: None,
        org_role: None,
    };
    let (status, _) = crate::handlers::console::run_console_cell::<SqliteDb>(
        Extension(db.clone()),
        Extension(events()),
        Extension(member),
        Path(cell_id),
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    let _ = Capability::ConsoleUse;

    let _ = std::fs::remove_file(db_path);
}
