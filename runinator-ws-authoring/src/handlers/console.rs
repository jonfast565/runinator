//! the rexrap console's http surface.
//!
//! sessions and cells are ordinary crud; the one endpoint that does anything is `POST
//! /console/cells/{id}/run`, and even that mostly delegates — the classifier decides what the cell
//! is, and the engine either evaluates it or starts a run.
//!
//! it lives in `-authoring` rather than `-runtime` because the console is an authoring surface: it
//! is where someone works out what a workflow should say. that a cell may start a run is incidental,
//! the same way saving a workflow may materialize a trigger.

use std::sync::Arc;

use axum::{Extension, Json, extract::Path, http::StatusCode};
use runinator_models::{
    auth::{AuthContext, Permission, ResourceType},
    console::{ConsoleSession, NewConsoleCell},
    rbac::{Action, ScopeKind, ScopeRef},
    validation::{SHORT_TEXT_MAX, Validate, ValidationError, optional_text},
};
use runinator_store::{
    RuntimeStore,
    roles::{
        ConsoleStore, DefinitionStore, FunctionStore, NotificationStore, ScheduleStore,
        WorkflowVmStore,
    },
};
use serde::Deserialize;
use uuid::Uuid;

use runinator_engine::services::ConsoleOperations;
use runinator_ws_core::ValidatedJson;
use runinator_ws_core::models::ApiResponse;
use runinator_ws_core::openapi::docs::{EndpointDoc, Example, endpoint, json_body};
use runinator_ws_core::responses::{api_error, bad_request, not_found};
use runinator_ws_middleware::authz::{AuthContextExt, AuthorizationStore, AuthzChecker};

fn selected_scope(ctx: &AuthContext) -> ScopeRef {
    ctx.org_id
        .and_then(|id| ScopeRef::new(ScopeKind::Organization, Some(id)))
        .unwrap_or(ScopeRef::PLATFORM)
}

/// what a caller sends to create or rename a session.
#[derive(Debug, Deserialize)]
pub struct ConsoleSessionRequest {
    #[serde(default)]
    pub name: Option<String>,
}

impl Validate for ConsoleSessionRequest {
    fn validate(&self) -> Result<(), ValidationError> {
        optional_text("name", self.name.as_deref(), SHORT_TEXT_MAX)
    }
}

fn session_name(request: &ConsoleSessionRequest) -> String {
    request
        .name
        .as_deref()
        .map(str::trim)
        .filter(|name| !name.is_empty())
        .unwrap_or("scratch")
        .to_string()
}

/// list the caller's sessions.
pub async fn get_console_sessions<
    T: AuthorizationStore
        + ConsoleStore
        + RuntimeStore
        + DefinitionStore
        + FunctionStore
        + NotificationStore
        + ScheduleStore
        + WorkflowVmStore,
>(
    Extension(db): Extension<Arc<T>>,
    Extension(console): Extension<Arc<ConsoleOperations<T>>>,
    Extension(ctx): Extension<AuthContext>,
) -> (StatusCode, Json<ApiResponse>) {
    if let Err(reply) = ctx.require_scope_action(Action::ConsoleUse, selected_scope(&ctx)) {
        return reply;
    }
    let visible = match AuthzChecker::new(db.as_ref(), &ctx)
        .visible_resource_ids(ResourceType::ConsoleSession)
        .await
    {
        Ok(visible) => visible,
        Err(reply) => return reply,
    };
    match console.list_sessions().await {
        Ok(sessions) => {
            let sessions: Vec<ConsoleSession> = sessions
                .into_iter()
                .filter(|session| visible.as_ref().is_none_or(|ids| ids.contains(&session.id)))
                .collect();
            (
                StatusCode::OK,
                Json(ApiResponse::ConsoleSessionList(sessions)),
            )
        }
        Err(err) => api_error(err.to_string()),
    }
}

/// create a session.
pub async fn create_console_session<
    T: AuthorizationStore
        + ConsoleStore
        + RuntimeStore
        + DefinitionStore
        + FunctionStore
        + NotificationStore
        + ScheduleStore
        + WorkflowVmStore,
>(
    Extension(db): Extension<Arc<T>>,
    Extension(console): Extension<Arc<ConsoleOperations<T>>>,
    Extension(ctx): Extension<AuthContext>,
    ValidatedJson(request): ValidatedJson<ConsoleSessionRequest>,
) -> (StatusCode, Json<ApiResponse>) {
    if let Err(reply) = ctx.require_scope_action(Action::ConsoleUse, selected_scope(&ctx)) {
        return reply;
    }
    match console
        .create_session(ctx.org_id, &session_name(&request), ctx.principal_id)
        .await
    {
        Ok(session) => {
            if let Err(reply) = AuthzChecker::new(db.as_ref(), &ctx)
                .grant_resource_owner(ResourceType::ConsoleSession, session.id)
                .await
            {
                return reply;
            }
            (StatusCode::OK, Json(ApiResponse::ConsoleSession(session)))
        }
        Err(err) => api_error(err.to_string()),
    }
}

/// one session with its cells and scope.
pub async fn get_console_session<
    T: AuthorizationStore
        + ConsoleStore
        + RuntimeStore
        + DefinitionStore
        + FunctionStore
        + NotificationStore
        + ScheduleStore
        + WorkflowVmStore,
>(
    Extension(db): Extension<Arc<T>>,
    Extension(console): Extension<Arc<ConsoleOperations<T>>>,
    Extension(ctx): Extension<AuthContext>,
    Path(session_id): Path<Uuid>,
) -> (StatusCode, Json<ApiResponse>) {
    if let Err(reply) = AuthzChecker::new(db.as_ref(), &ctx)
        .require_resource(ResourceType::ConsoleSession, session_id, Permission::View)
        .await
    {
        return reply;
    }
    match console.session_detail(session_id).await {
        Ok(Some(detail)) => (
            StatusCode::OK,
            Json(ApiResponse::ConsoleSessionDetail(Box::new(detail))),
        ),
        Ok(None) => not_found(format!("console session {session_id} not found")),
        Err(err) => api_error(err.to_string()),
    }
}

/// rename a session.
pub async fn rename_console_session<
    T: AuthorizationStore
        + ConsoleStore
        + RuntimeStore
        + DefinitionStore
        + FunctionStore
        + NotificationStore
        + ScheduleStore
        + WorkflowVmStore,
>(
    Extension(db): Extension<Arc<T>>,
    Extension(console): Extension<Arc<ConsoleOperations<T>>>,
    Extension(ctx): Extension<AuthContext>,
    Path(session_id): Path<Uuid>,
    ValidatedJson(request): ValidatedJson<ConsoleSessionRequest>,
) -> (StatusCode, Json<ApiResponse>) {
    if let Err(reply) = require_session(db.as_ref(), &ctx, session_id, Permission::Edit).await {
        return reply;
    }
    match console
        .rename_session(session_id, &session_name(&request))
        .await
    {
        Ok(true) => {
            get_console_session(
                Extension(db),
                Extension(console),
                Extension(ctx),
                Path(session_id),
            )
            .await
        }
        Ok(false) => not_found(format!("console session {session_id} not found")),
        Err(err) => api_error(err.to_string()),
    }
}

/// delete a session and everything under it.
pub async fn delete_console_session<
    T: AuthorizationStore
        + ConsoleStore
        + RuntimeStore
        + DefinitionStore
        + FunctionStore
        + NotificationStore
        + ScheduleStore
        + WorkflowVmStore,
>(
    Extension(db): Extension<Arc<T>>,
    Extension(console): Extension<Arc<ConsoleOperations<T>>>,
    Extension(ctx): Extension<AuthContext>,
    Path(session_id): Path<Uuid>,
) -> (StatusCode, Json<ApiResponse>) {
    if let Err(reply) = require_session(db.as_ref(), &ctx, session_id, Permission::Own).await {
        return reply;
    }
    match console.delete_session(session_id).await {
        Ok(true) => (
            StatusCode::OK,
            Json(ApiResponse::JsonValue(runinator_models::json!({
                "deleted": true,
                "session_id": session_id,
            }))),
        ),
        Ok(false) => not_found(format!("console session {session_id} not found")),
        Err(err) => api_error(err.to_string()),
    }
}

/// remove every persisted cell and scope entry while retaining the session.
pub async fn clear_console_session<
    T: AuthorizationStore
        + ConsoleStore
        + RuntimeStore
        + DefinitionStore
        + FunctionStore
        + NotificationStore
        + ScheduleStore
        + WorkflowVmStore,
>(
    Extension(db): Extension<Arc<T>>,
    Extension(console): Extension<Arc<ConsoleOperations<T>>>,
    Extension(ctx): Extension<AuthContext>,
    Path(session_id): Path<Uuid>,
) -> (StatusCode, Json<ApiResponse>) {
    if let Err(reply) = require_session(db.as_ref(), &ctx, session_id, Permission::Edit).await {
        return reply;
    }
    match console.clear_session(session_id).await {
        Ok(true) => (
            StatusCode::OK,
            Json(ApiResponse::JsonValue(runinator_models::json!({
                "cleared": true,
                "session_id": session_id,
            }))),
        ),
        Ok(false) => not_found(format!("console session {session_id} not found")),
        Err(err) => api_error(err.to_string()),
    }
}

/// append a cell to a session.
pub async fn create_console_cell<
    T: AuthorizationStore
        + ConsoleStore
        + RuntimeStore
        + DefinitionStore
        + FunctionStore
        + NotificationStore
        + ScheduleStore
        + WorkflowVmStore,
>(
    Extension(db): Extension<Arc<T>>,
    Extension(console): Extension<Arc<ConsoleOperations<T>>>,
    Extension(ctx): Extension<AuthContext>,
    Path(session_id): Path<Uuid>,
    ValidatedJson(cell): ValidatedJson<NewConsoleCell>,
) -> (StatusCode, Json<ApiResponse>) {
    if let Err(reply) = require_session(db.as_ref(), &ctx, session_id, Permission::Edit).await {
        return reply;
    }
    match console.upsert_cell(session_id, None, &cell).await {
        Ok(cell) => (StatusCode::OK, Json(ApiResponse::ConsoleCell(cell))),
        Err(err) => api_error(err.to_string()),
    }
}

/// replace a cell's source.
pub async fn update_console_cell<
    T: AuthorizationStore
        + ConsoleStore
        + RuntimeStore
        + DefinitionStore
        + FunctionStore
        + NotificationStore
        + ScheduleStore
        + WorkflowVmStore,
>(
    Extension(db): Extension<Arc<T>>,
    Extension(console): Extension<Arc<ConsoleOperations<T>>>,
    Extension(ctx): Extension<AuthContext>,
    Path(cell_id): Path<Uuid>,
    ValidatedJson(request): ValidatedJson<NewConsoleCell>,
) -> (StatusCode, Json<ApiResponse>) {
    let cell = match require_cell(
        db.as_ref(),
        console.as_ref(),
        &ctx,
        cell_id,
        Permission::Edit,
    )
    .await
    {
        Ok(cell) => cell,
        Err(reply) => return reply,
    };
    if cell.status == runinator_models::console::ConsoleCellStatus::Running {
        return bad_request("a running console cell must be canceled before it can be edited");
    }
    match console
        .upsert_cell(cell.session_id, Some(cell_id), &request)
        .await
    {
        Ok(cell) => (StatusCode::OK, Json(ApiResponse::ConsoleCell(cell))),
        Err(err) => api_error(err.to_string()),
    }
}

/// delete a cell.
pub async fn delete_console_cell<
    T: AuthorizationStore
        + ConsoleStore
        + RuntimeStore
        + DefinitionStore
        + FunctionStore
        + NotificationStore
        + ScheduleStore
        + WorkflowVmStore,
>(
    Extension(db): Extension<Arc<T>>,
    Extension(console): Extension<Arc<ConsoleOperations<T>>>,
    Extension(ctx): Extension<AuthContext>,
    Path(cell_id): Path<Uuid>,
) -> (StatusCode, Json<ApiResponse>) {
    let cell = match require_cell(
        db.as_ref(),
        console.as_ref(),
        &ctx,
        cell_id,
        Permission::Edit,
    )
    .await
    {
        Ok(cell) => cell,
        Err(reply) => return reply,
    };
    if cell.status == runinator_models::console::ConsoleCellStatus::Running {
        return bad_request("a running console cell must be canceled before it can be deleted");
    }
    match console.delete_cell(cell_id).await {
        Ok(true) => (
            StatusCode::OK,
            Json(ApiResponse::JsonValue(runinator_models::json!({
                "deleted": true,
                "cell_id": cell_id,
            }))),
        ),
        Ok(false) => not_found(format!("console cell {cell_id} not found")),
        Err(err) => api_error(err.to_string()),
    }
}

/// run one cell.
///
/// the response is the cell, not a run: a pure cell has already settled by the time this returns,
/// and an effectful one carries its `workflow_run_id` for the caller to follow. one shape either
/// way, so the UI does not branch on how the cell happened to be written.
pub async fn run_console_cell<
    T: AuthorizationStore
        + ConsoleStore
        + RuntimeStore
        + DefinitionStore
        + FunctionStore
        + NotificationStore
        + ScheduleStore
        + WorkflowVmStore,
>(
    Extension(db): Extension<Arc<T>>,
    Extension(console): Extension<Arc<ConsoleOperations<T>>>,
    Extension(ctx): Extension<AuthContext>,
    Path(cell_id): Path<Uuid>,
) -> (StatusCode, Json<ApiResponse>) {
    let cell = match require_cell(
        db.as_ref(),
        console.as_ref(),
        &ctx,
        cell_id,
        Permission::Run,
    )
    .await
    {
        Ok(cell) => cell,
        Err(reply) => return reply,
    };
    if cell.status == runinator_models::console::ConsoleCellStatus::Running {
        return bad_request("console cell is already running");
    }
    match console.run_cell(cell_id).await {
        Ok(outcome) => (StatusCode::OK, Json(ApiResponse::ConsoleCell(outcome.cell))),
        Err(err) => api_error(err.to_string()),
    }
}

/// replay a settled cell against the session's current binding snapshot.
pub async fn replay_console_cell<
    T: AuthorizationStore
        + ConsoleStore
        + RuntimeStore
        + DefinitionStore
        + FunctionStore
        + NotificationStore
        + ScheduleStore
        + WorkflowVmStore,
>(
    db: Extension<Arc<T>>,
    console: Extension<Arc<ConsoleOperations<T>>>,
    ctx: Extension<AuthContext>,
    cell_id: Path<Uuid>,
) -> (StatusCode, Json<ApiResponse>) {
    run_console_cell(db, console, ctx, cell_id).await
}

/// cancel the durable workflow behind an effectful cell.
pub async fn cancel_console_cell<
    T: AuthorizationStore
        + ConsoleStore
        + RuntimeStore
        + DefinitionStore
        + FunctionStore
        + NotificationStore
        + ScheduleStore
        + WorkflowVmStore,
>(
    Extension(db): Extension<Arc<T>>,
    Extension(console): Extension<Arc<ConsoleOperations<T>>>,
    Extension(ctx): Extension<AuthContext>,
    Path(cell_id): Path<Uuid>,
) -> (StatusCode, Json<ApiResponse>) {
    let cell = match require_cell(
        db.as_ref(),
        console.as_ref(),
        &ctx,
        cell_id,
        Permission::Run,
    )
    .await
    {
        Ok(cell) => cell,
        Err(reply) => return reply,
    };
    let Some(run_id) = cell.workflow_run_id else {
        return bad_request("console cell has no workflow run to cancel");
    };
    match console.cancel_cell_run(run_id).await {
        Ok(response) => (StatusCode::OK, Json(ApiResponse::TaskResponse(response))),
        Err(err) => api_error(err.to_string()),
    }
}

/// re-read a cell, which is how a caller polls one waiting on a scratch run.
pub async fn get_console_cell<
    T: AuthorizationStore
        + ConsoleStore
        + RuntimeStore
        + DefinitionStore
        + FunctionStore
        + NotificationStore
        + ScheduleStore
        + WorkflowVmStore,
>(
    Extension(db): Extension<Arc<T>>,
    Extension(console): Extension<Arc<ConsoleOperations<T>>>,
    Extension(ctx): Extension<AuthContext>,
    Path(cell_id): Path<Uuid>,
) -> (StatusCode, Json<ApiResponse>) {
    let cell = match require_cell(
        db.as_ref(),
        console.as_ref(),
        &ctx,
        cell_id,
        Permission::View,
    )
    .await
    {
        Ok(cell) => cell,
        Err(reply) => return reply,
    };
    // settle it from its run first: the VM records the run, and this is where a finished run
    // becomes a finished cell. a poll that only read the row would show `running` forever.
    if let Some(run_id) = cell.workflow_run_id {
        match console.settle_cell_for_run(run_id).await {
            Ok(Some(settled)) => return (StatusCode::OK, Json(ApiResponse::ConsoleCell(settled))),
            Ok(None) => {}
            Err(err) => return api_error(err.to_string()),
        }
    }
    (StatusCode::OK, Json(ApiResponse::ConsoleCell(cell)))
}

// a session the caller may see, or the reply that says otherwise.
async fn require_session<
    T: AuthorizationStore
        + ConsoleStore
        + RuntimeStore
        + DefinitionStore
        + FunctionStore
        + NotificationStore
        + ScheduleStore
        + WorkflowVmStore,
>(
    db: &T,
    ctx: &AuthContext,
    session_id: Uuid,
    needed: Permission,
) -> Result<(), (StatusCode, Json<ApiResponse>)> {
    AuthzChecker::new(db, ctx)
        .require_resource(ResourceType::ConsoleSession, session_id, needed)
        .await
}

async fn require_cell<
    T: AuthorizationStore
        + ConsoleStore
        + RuntimeStore
        + DefinitionStore
        + FunctionStore
        + NotificationStore
        + ScheduleStore
        + WorkflowVmStore,
>(
    db: &T,
    console: &ConsoleOperations<T>,
    ctx: &AuthContext,
    cell_id: Uuid,
    needed: Permission,
) -> Result<runinator_models::console::ConsoleCell, (StatusCode, Json<ApiResponse>)> {
    let cell = match console.fetch_cell(cell_id).await {
        Ok(Some(cell)) => cell,
        Ok(None) => return Err(not_found(format!("console cell {cell_id} not found"))),
        Err(err) => return Err(api_error(err.to_string())),
    };
    // authorized through the owning session, so a cell id alone never reveals another org's work.
    require_session(db, ctx, cell.session_id, needed).await?;
    Ok(cell)
}

/// the `console` endpoints.
pub fn routes<
    T: AuthorizationStore
        + ConsoleStore
        + RuntimeStore
        + DefinitionStore
        + FunctionStore
        + NotificationStore
        + ScheduleStore
        + WorkflowVmStore,
>(
    pool: std::sync::Arc<T>,
) -> axum::Router {
    use axum::Extension;
    use axum::routing::{get, post};
    axum::Router::new()
        .route(
            "/console/sessions",
            get(get_console_sessions::<T>)
                .post(create_console_session::<T>)
                .layer(Extension(pool.clone())),
        )
        .route(
            "/console/sessions/{id}",
            get(get_console_session::<T>)
                .patch(rename_console_session::<T>)
                .delete(delete_console_session::<T>)
                .layer(Extension(pool.clone())),
        )
        .route(
            "/console/sessions/{id}/cells",
            post(create_console_cell::<T>).layer(Extension(pool.clone())),
        )
        .route(
            "/console/sessions/{id}/clear",
            post(clear_console_session::<T>).layer(Extension(pool.clone())),
        )
        .route(
            "/console/cells/{id}",
            get(get_console_cell::<T>)
                .patch(update_console_cell::<T>)
                .delete(delete_console_cell::<T>)
                .layer(Extension(pool.clone())),
        )
        .route(
            "/console/cells/{id}/run",
            post(run_console_cell::<T>).layer(Extension(pool.clone())),
        )
        .route(
            "/console/cells/{id}/cancel",
            post(cancel_console_cell::<T>).layer(Extension(pool.clone())),
        )
        .route(
            "/console/cells/{id}/replay",
            post(replay_console_cell::<T>).layer(Extension(pool.clone())),
        )
}

/// the openapi entries for the routes above.
pub const DOCS: &[EndpointDoc] = &[
    endpoint(
        "get",
        "/console/sessions",
        "Console",
        "List console sessions",
        "Lists the REXRAP console sessions visible to the caller.",
        false,
        None,
        &[],
        200,
        "console sessions",
        Example::None,
    ),
    endpoint(
        "post",
        "/console/sessions",
        "Console",
        "Create a console session",
        "Creates a REXRAP console session: a notebook of cells sharing one scope.",
        false,
        json_body("Optional session name.", Example::None),
        &[],
        200,
        "console session",
        Example::None,
    ),
    endpoint(
        "get",
        "/console/sessions/{id}",
        "Console",
        "Read a console session",
        "Returns one session with its cells and the scope they share.",
        false,
        None,
        &[],
        200,
        "console session",
        Example::None,
    ),
    endpoint(
        "patch",
        "/console/sessions/{id}",
        "Console",
        "Rename a console session",
        "Renames a console session.",
        false,
        json_body("New session name.", Example::None),
        &[],
        200,
        "console session",
        Example::None,
    ),
    endpoint(
        "delete",
        "/console/sessions/{id}",
        "Console",
        "Delete a console session",
        "Deletes a session, its cells, and its scope.",
        false,
        None,
        &[],
        200,
        "session deleted",
        Example::None,
    ),
    endpoint(
        "post",
        "/console/sessions/{id}/clear",
        "Console",
        "Clear a console session",
        "Removes a session's cells, scope bindings, and function library but retains the session.",
        false,
        None,
        &[],
        200,
        "session cleared",
        Example::None,
    ),
    endpoint(
        "post",
        "/console/sessions/{id}/cells",
        "Console",
        "Append a console cell",
        "Appends a cell to a session.",
        false,
        json_body("Cell source and optional label.", Example::None),
        &[],
        200,
        "console cell",
        Example::None,
    ),
    endpoint(
        "get",
        "/console/cells/{id}",
        "Console",
        "Read a console cell",
        "Returns one cell, settling it first if it was waiting on a scratch workflow run.",
        false,
        None,
        &[],
        200,
        "console cell",
        Example::None,
    ),
    endpoint(
        "patch",
        "/console/cells/{id}",
        "Console",
        "Edit a console cell",
        "Replaces a cell's source. Its previous result is cleared, since a result beside changed \
         source is a stale answer shown as a current one.",
        false,
        json_body("Cell source and optional label.", Example::None),
        &[],
        200,
        "console cell",
        Example::None,
    ),
    endpoint(
        "delete",
        "/console/cells/{id}",
        "Console",
        "Delete a console cell",
        "Deletes a cell and the binding it produced.",
        false,
        None,
        &[],
        200,
        "cell deleted",
        Example::None,
    ),
    endpoint(
        "post",
        "/console/cells/{id}/run",
        "Console",
        "Run a console cell",
        "Runs one cell. A pure expression is evaluated in process and has already settled when \
         this returns; anything effectful becomes a scratch workflow run, and the cell carries its \
         `workflow_run_id` to follow.",
        false,
        None,
        &[],
        200,
        "console cell",
        Example::None,
    ),
    endpoint(
        "post",
        "/console/cells/{id}/cancel",
        "Console",
        "Cancel a running console cell",
        "Cancels the scratch workflow run a cell started. A cell that settled in process has no \
         run to cancel and is left as it is.",
        false,
        None,
        &[],
        200,
        "console cell",
        Example::None,
    ),
    endpoint(
        "post",
        "/console/cells/{id}/replay",
        "Console",
        "Replay a settled console cell",
        "Runs a cell again against the session's current bindings, which is how a cell is \
         re-evaluated after an earlier cell in the notebook changed what it reads.",
        false,
        None,
        &[],
        200,
        "console cell",
        Example::None,
    ),
];
