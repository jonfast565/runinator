//! invoking a packaged function over http.
//!
//! this endpoint starts a *workflow run* of the export's generated adapter rather than executing a
//! container itself. retry, timeout, cancellation, logs, artifacts, tracing, and the debugger are
//! all properties of a run; a second execution path would need its own copy of every one and would
//! drift from the workflow path immediately. so "invoke over http" and "call it from a workflow"
//! are the same machinery reached two ways, which is what makes them behave identically.
//!
//! the response shape is a request-level choice. a caller that sends `Prefer: respond-async` always
//! gets 202 and a run id; otherwise the handler waits a bounded moment for a terminal status and
//! returns the output inline, falling back to 202 when the run is still going. it is a short poll
//! rather than a long-poll: nothing server-side blocks on a run today, and inventing that here would
//! put a held connection in front of the VM.

use std::sync::Arc;
use std::time::Duration;

use axum::{
    Extension, Json,
    extract::{Path, Query},
    http::{HeaderMap, StatusCode},
};
use runinator_database::interfaces::DatabaseImpl;
use runinator_models::{
    auth::{AuthContext, Permission, ResourceType},
    functions::{DEFAULT_ALIAS, FunctionVersionRef},
    replicas::{TriggerActorType, TriggerSourceKind, WorkflowRunProvenance},
    value::Value,
    workflows::WorkflowRun,
};
use serde::Deserialize;
use uuid::Uuid;

use runinator_engine::repository;
use runinator_ws_core::events::{EventSender, emit_workflow_run, nudge_wake_publisher};
use runinator_ws_core::models::{self, ApiResponse};
use runinator_ws_core::openapi::docs::{EndpointDoc, Example, endpoint, json_body};
use runinator_ws_core::responses::{api_error, bad_request, not_found};
use runinator_ws_middleware::authz::AuthzChecker;

/// how long a synchronous invocation waits before falling back to 202.
const SYNC_WAIT: Duration = Duration::from_secs(5);
/// how often it re-reads the run while waiting.
const SYNC_POLL: Duration = Duration::from_millis(200);

/// the header a caller sends to skip the wait entirely.
const PREFER_HEADER: &str = "prefer";
const PREFER_ASYNC: &str = "respond-async";
/// scoped per org and export, so two orgs can use the same key without colliding.
const IDEMPOTENCY_HEADER: &str = "idempotency-key";
const IDEMPOTENCY_SCOPE: &str = "function_invocation";

/// which version to invoke.
#[derive(Debug, Default, Deserialize)]
pub struct InvocationQuery {
    /// an alias, defaulting to `latest`. resolved *now*, so an invocation follows a promotion.
    pub alias: Option<String>,
    /// or an exact version number, which no promotion moves.
    pub version: Option<i64>,
}

/// start one invocation.
pub async fn create_function_invocation<T: DatabaseImpl>(
    Extension(db): Extension<Arc<T>>,
    Extension(events): Extension<EventSender>,
    Extension(ctx): Extension<AuthContext>,
    headers: HeaderMap,
    Path((package, export)): Path<(String, String)>,
    Query(query): Query<InvocationQuery>,
    Json(input): Json<Value>,
) -> (StatusCode, Json<ApiResponse>) {
    let (namespace, name) = split_qualified(&package);
    let Some(detail) = (match repository::functions::fetch_package_detail(
        db.as_ref(),
        ctx.org_id,
        namespace.as_deref(),
        &name,
    )
    .await
    {
        Ok(detail) => detail,
        Err(err) => return api_error(err.to_string()),
    }) else {
        return not_found(format!("function package '{package}' not found"));
    };
    if let Err(reply) = AuthzChecker::new(db.as_ref(), &ctx)
        .require_resource(
            ResourceType::FunctionPackage,
            detail.package.id,
            Permission::Run,
        )
        .await
    {
        return reply;
    }

    // the alias resolves at call time, which is exactly the difference between this path and a
    // compiled workflow: an http caller asking for `production` means whatever `production` is now,
    // while a workflow pinned its version when it compiled.
    let reference = match (&query.version, &query.alias) {
        (Some(_), Some(_)) => return bad_request("name a version or an alias, not both"),
        (Some(version), None) => FunctionVersionRef::Exact(*version),
        (None, Some(alias)) => FunctionVersionRef::Alias(alias.clone()),
        (None, None) => FunctionVersionRef::Alias(DEFAULT_ALIAS.to_string()),
    };
    let resolved =
        repository::functions::resolve_export(db.as_ref(), &detail.package, &reference, &export)
            .await;
    let (version, resolved_export) = match resolved {
        Ok(resolved) => resolved,
        Err(err) => return not_found(err.to_string()),
    };

    let Some(adapter) = (match repository::function_adapters::fetch_adapter_workflow(
        db.as_ref(),
        resolved_export.id,
    )
    .await
    {
        Ok(adapter) => adapter,
        Err(err) => return api_error(err.to_string()),
    }) else {
        return api_error(format!(
            "'{package}.{export}' has no adapter workflow; republish the package"
        ));
    };

    // authorized against the adapter workflow, so a function grant is an ordinary workflow grant
    // and nothing here invents a second permission model.
    if let Err(reply) = AuthzChecker::new(db.as_ref(), &ctx)
        .require_workflow(adapter.workflow_id, Permission::Run)
        .await
    {
        return reply;
    }

    // an idempotency key replays the run it already started rather than starting a second one. the
    // scope carries the org and the export, so the same key means different things to different
    // callers and cannot leak one org's run id to another.
    let idempotency_key = header(&headers, IDEMPOTENCY_HEADER);
    if let Some(key) = &idempotency_key {
        let scope = idempotency_scope(ctx.org_id, &package, &export);
        match repository::fetch_idempotency_key(db.as_ref(), scope, key.clone()).await {
            Ok(Some(stored)) => {
                if let Some(run_id) = stored
                    .pointer("/result/workflow_run_id")
                    .and_then(Value::as_str)
                    .and_then(|raw| raw.parse::<Uuid>().ok())
                {
                    return replay(db.as_ref(), run_id).await;
                }
            }
            Ok(None) => {}
            Err(err) => return api_error(err.to_string()),
        }
    }

    let provenance = WorkflowRunProvenance {
        source_kind: Some(TriggerSourceKind::Function),
        actor_type: Some(TriggerActorType::User),
        actor_display_name: Some(format!("{package}.{export}")),
        metadata: runinator_models::json!({
            "package": package,
            "export": export,
            "version": version.version,
        }),
        ..Default::default()
    };
    let run = match repository::create_workflow_run(
        db.as_ref(),
        adapter.workflow_id,
        input,
        false,
        Some(format!("{package}.{export} v{}", version.version)),
        provenance,
    )
    .await
    {
        Ok(run) => run,
        Err(err) => return api_error(err.to_string()),
    };
    let org_id = repository::org_id_for_workflow_run(db.as_ref(), run.id).await;
    emit_workflow_run(&events, run.id, org_id);
    nudge_wake_publisher(&events);

    if let Some(key) = &idempotency_key {
        let scope = idempotency_scope(ctx.org_id, &package, &export);
        // recorded after the run exists, so a stored key always names a run that was really started.
        let _ = repository::put_idempotency_key(
            db.as_ref(),
            scope,
            key.clone(),
            runinator_models::json!({ "workflow_run_id": run.id }),
        )
        .await;
    }

    if prefers_async(&headers) {
        return accepted(run);
    }
    settle_or_accept(db.as_ref(), run).await
}

/// the status of one invocation, which is the status of its run.
pub async fn get_function_invocation<T: DatabaseImpl>(
    Extension(db): Extension<Arc<T>>,
    Extension(ctx): Extension<AuthContext>,
    Path(run_id): Path<Uuid>,
) -> (StatusCode, Json<ApiResponse>) {
    if let Err(reply) = require_run_access(db.as_ref(), &ctx, run_id, Permission::View).await {
        return reply;
    }
    replay(db.as_ref(), run_id).await
}

/// cancel one invocation.
pub async fn cancel_function_invocation<T: DatabaseImpl>(
    Extension(db): Extension<Arc<T>>,
    Extension(broker): Extension<Arc<dyn runinator_broker_core::Broker>>,
    Extension(events): Extension<EventSender>,
    Extension(ctx): Extension<AuthContext>,
    Path(run_id): Path<Uuid>,
) -> (StatusCode, Json<ApiResponse>) {
    if let Err(reply) = require_run_access(db.as_ref(), &ctx, run_id, Permission::Run).await {
        return reply;
    }
    // delegates to the ordinary run-cancel path rather than reimplementing it: an invocation is a
    // run, and a second cancel implementation is a second set of edge cases.
    match repository::cancel_workflow_run(db.as_ref(), broker.as_ref(), run_id).await {
        Ok(_) => {
            nudge_wake_publisher(&events);
            replay(db.as_ref(), run_id).await
        }
        Err(err) => api_error(err.to_string()),
    }
}

// wait a bounded moment for a terminal status, then answer with the output or with 202.
async fn settle_or_accept<T: DatabaseImpl>(
    db: &T,
    run: WorkflowRun,
) -> (StatusCode, Json<ApiResponse>) {
    let deadline = std::time::Instant::now() + SYNC_WAIT;
    loop {
        if std::time::Instant::now() >= deadline {
            return accepted(run);
        }
        tokio::time::sleep(SYNC_POLL).await;
        match repository::fetch_workflow_run(db, run.id).await {
            Ok(Some(current)) if current.status.is_terminal() => {
                return replay(db, current.id).await;
            }
            Ok(_) => continue,
            // a read failure mid-wait is not the invocation's failure: the run exists and is going,
            // so report it as accepted rather than as an error the caller might retry into a second
            // run.
            Err(_) => return accepted(run),
        }
    }
}

fn accepted(run: WorkflowRun) -> (StatusCode, Json<ApiResponse>) {
    (
        StatusCode::ACCEPTED,
        Json(ApiResponse::WorkflowRun(models::WorkflowRunResponse::new(
            run,
            Vec::new(),
        ))),
    )
}

// Read the VM-backed run. Effects and journal entries have their own resources.
async fn replay<T: DatabaseImpl>(db: &T, run_id: Uuid) -> (StatusCode, Json<ApiResponse>) {
    let run = match repository::fetch_workflow_run(db, run_id).await {
        Ok(Some(found)) => found,
        Ok(None) => return not_found(format!("invocation {run_id} not found")),
        Err(err) => return api_error(err.to_string()),
    };
    let status = if run.status.is_terminal() {
        StatusCode::OK
    } else {
        StatusCode::ACCEPTED
    };
    (
        status,
        Json(ApiResponse::WorkflowRun(models::WorkflowRunResponse::new(
            run,
            Vec::new(),
        ))),
    )
}

async fn require_run_access<T: DatabaseImpl>(
    db: &T,
    ctx: &AuthContext,
    run_id: Uuid,
    permission: Permission,
) -> Result<(), (StatusCode, Json<ApiResponse>)> {
    let run = match repository::fetch_workflow_run(db, run_id).await {
        Ok(Some(found)) => found,
        Ok(None) => return Err(not_found(format!("invocation {run_id} not found"))),
        Err(err) => return Err(api_error(err.to_string())),
    };
    AuthzChecker::new(db, ctx)
        .require_workflow(run.workflow_id, permission)
        .await
}

fn prefers_async(headers: &HeaderMap) -> bool {
    header(headers, PREFER_HEADER)
        .is_some_and(|value| value.to_ascii_lowercase().contains(PREFER_ASYNC))
}

fn header(headers: &HeaderMap, name: &str) -> Option<String> {
    headers
        .get(name)
        .and_then(|value| value.to_str().ok())
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
}

fn idempotency_scope(org_id: Option<Uuid>, package: &str, export: &str) -> String {
    let org = org_id
        .map(|id| id.to_string())
        .unwrap_or_else(|| "global".to_string());
    format!("{IDEMPOTENCY_SCOPE}:{org}:{package}.{export}")
}

// `namespace.name` or a bare `name`; names cannot contain dots, so this split is unambiguous.
fn split_qualified(package: &str) -> (Option<String>, String) {
    match package.split_once('.') {
        Some((namespace, name)) => (Some(namespace.to_string()), name.to_string()),
        None => (None, package.to_string()),
    }
}

/// the function-invocation endpoints.
pub fn routes<T: DatabaseImpl>(pool: std::sync::Arc<T>) -> axum::Router {
    use axum::Extension;
    use axum::routing::{get, post};
    axum::Router::new()
        .route(
            "/functions/{package}/{export}/invocations",
            post(create_function_invocation::<T>).layer(Extension(pool.clone())),
        )
        .route(
            "/function_invocations/{run_id}",
            get(get_function_invocation::<T>).layer(Extension(pool.clone())),
        )
        .route(
            "/function_invocations/{run_id}/cancel",
            post(cancel_function_invocation::<T>).layer(Extension(pool.clone())),
        )
}

/// the openapi entries for the routes above.
pub const DOCS: &[EndpointDoc] = &[
    endpoint(
        "post",
        "/functions/{package}/{export}/invocations",
        "Functions",
        "Invoke a function",
        "Starts a run of the export's adapter workflow. Waits briefly for a terminal status and \
         returns the output inline; send `Prefer: respond-async` for an immediate 202. \
         `Idempotency-Key` replays the run an earlier identical request started.",
        false,
        json_body("The function's declared input.", Example::None),
        &[],
        200,
        "invocation completed, or 202 while it is still running",
        Example::WorkflowRun,
    ),
    endpoint(
        "get",
        "/function_invocations/{run_id}",
        "Functions",
        "Read an invocation",
        "Returns an invocation's run, including its node runs and output.",
        false,
        None,
        &[],
        200,
        "invocation",
        Example::WorkflowRun,
    ),
    endpoint(
        "post",
        "/function_invocations/{run_id}/cancel",
        "Functions",
        "Cancel an invocation",
        "Requests cancellation of a running invocation.",
        false,
        None,
        &[],
        200,
        "cancellation requested",
        Example::WorkflowRun,
    ),
];
