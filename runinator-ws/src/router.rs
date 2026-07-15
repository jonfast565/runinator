use std::sync::Arc;

use axum::response::IntoResponse;
use axum::{
    Extension, Router,
    extract::DefaultBodyLimit,
    middleware::from_fn_with_state,
    routing::{delete, get, patch, post},
};
use runinator_broker::Broker;
use runinator_database::interfaces::DatabaseImpl;
use runinator_models::api_routes::{
    API_ARTIFACTS, API_PACKS_IMPORT, API_PIPELINES, API_PROVIDERS, API_REPLICAS, API_RUNS,
    API_SCHEDULER_ACTION_DISPATCHES, API_SCHEDULER_ACTION_DISPATCHES_CLAIM,
    API_SCHEDULER_ACTION_DISPATCHES_PENDING, API_SCHEDULER_READY_NODES_CLAIM,
    API_SCHEDULER_WORKFLOW_RUNS_CLAIM, API_SCHEDULER_WORKFLOW_TRIGGER_FIRINGS_CLAIM,
    API_WDL_ANALYZE, API_WDL_COMPILE, API_WDL_COMPLETE, API_WDL_DECOMPILE, API_WDL_EVALUATE,
    API_WDL_FORMAT, API_WDL_HOVER, API_WDL_IMPORT, API_WORKFLOW_RUNS, API_WORKFLOW_TRIGGERS_DUE,
    API_WORKFLOWS, API_WORKFLOWS_EXPORT, API_WORKFLOWS_IMPORT, API_WORKFLOWS_VALIDATE,
};
use runinator_provisioner::ProvisionerRegistry;
use tower_http::catch_panic::CatchPanicLayer;
use tower_http::cors::{Any, CorsLayer};

use crate::models::{ApiError, ApiResponse};

use crate::auth::{AuthConfig, AuthState, auth_middleware};
use crate::events::EventSender;
use crate::handlers::{
    action_dispatches::{
        claim_action_dispatches, enqueue_action_dispatch, mark_action_dispatch_failed,
        mark_action_dispatch_published, pending_action_dispatches,
    },
    artifacts::{
        add_run_artifact, delete_artifact, download_artifact, get_run_artifacts, list_artifacts,
        upload_artifact,
    },
    auth::{
        add_team_member, auth_config, create_api_key, create_team, create_user,
        create_workflow_grant, delete_team, delete_user, list_api_keys, list_team_members,
        list_teams, list_user_teams, list_users, list_workflow_grants, login, logout, me, refresh,
        remove_team_member, revoke_api_key, revoke_workflow_grant, rotate_api_key, update_api_key,
        update_team, update_user,
    },
    automation::{
        approve_request, close_gate, create_approval, create_automation_event,
        create_external_item, create_gate, delete_automation_event, delete_gate, get_approvals,
        get_automation_events, get_external_items, get_gate, get_gates, get_idempotency_key,
        open_gate, put_idempotency_key, reject_request,
    },
    billing::{
        get_org_nodes, get_org_quota, get_org_usage, get_rate_card, put_org_quota, scale_org_nodes,
    },
    catalog::{get_catalog_items, upsert_catalog_item},
    catalog_metadata::{get_enum_catalogs, get_node_kinds, get_trigger_kinds},
    credentials::{
        delete_credential, get_credential, import_secret_bundle, put_credential, reencrypt_settings,
    },
    debug::{
        continue_debug_workflow_run, debug_command, rerun_debug_workflow_node,
        run_to_cursor_workflow_run, skip_debug_workflow_node, step_debug_workflow_run,
        update_workflow_run_debug,
    },
    health::{health, metrics, ready},
    node_runs::{
        add_workflow_node_run_artifact, append_workflow_node_run_chunk,
        claim_workflow_node_run_executor, create_workflow_node_run,
        get_workflow_node_run_artifacts, get_workflow_node_run_chunks, get_workflow_run_artifacts,
        release_workflow_node_run_executor, resolve_workflow_input, update_workflow_node_run,
    },
    notifications::{
        create_notification, delete_notification, list_notifications, mark_all_notifications_read,
        mark_notification_read,
    },
    observability::{get_audit_log, get_dead_letters},
    orgs::{
        add_org_member, create_org, delete_org, get_org, list_my_orgs, list_org_members, list_orgs,
        remove_org_member, switch_org, update_org, update_org_member,
    },
    packs::import_pack,
    pipelines::{
        create_pipeline, delete_pipeline, get_pipeline, get_pipelines, set_pipeline_owner,
        update_pipeline,
    },
    providers::{get_providers, import_provider_bundle, upsert_provider},
    provisioning::{get_node_backends, get_nodes, scale_nodes, stop_node},
    replicas::{
        get_replica_providers, get_replica_samples, get_replicas, heartbeat_replica,
        mark_replica_offline, register_replica, upsert_replica_provider,
    },
    runs::{
        append_run_chunk, cancel_workflow_run, claim_ready_nodes,
        claim_workflow_runs_for_scheduler, create_workflow_run, create_workflow_trigger_run,
        deliver_signal, get_run_chunks, get_runs, get_workflow_run, get_workflow_runs,
        pause_workflow_run, process_ready_node, release_workflow_run_claim, rename_workflow_run,
        renew_workflow_run_claim, replay_workflow_run, resume_workflow_run, update_run,
        update_workflow_run,
    },
    supervisor::get_supervisor_status,
    triggers::{
        claim_due_workflow_trigger_firings, delete_workflow_trigger, get_due_workflow_triggers,
        get_workflow_trigger, get_workflow_triggers, update_workflow_trigger,
        upsert_workflow_trigger,
    },
    wdl::{
        analyze_wdl, compile_wdl, complete_wdl, decompile_to_wdl, evaluate_expression, format_wdl,
        hover_wdl, import_wdl,
    },
    webhook::{webhook_signal, webhook_wake},
    workflows::{
        delete_workflow, duplicate_workflow, export_single_workflow_bundle, export_workflow_bundle,
        get_workflow, get_workflows, import_workflow_bundle, set_workflow_owner, upsert_workflow,
        validate_workflow,
    },
};
use crate::rate_limit::{RateLimitConfig, RateLimiter, rate_limit_middleware};
use crate::websocket::{
    ws_desktop_worker, ws_events, ws_run_stream, ws_workflow_node_run_stream, ws_workflow_run,
};

pub fn build_router<T: DatabaseImpl>(
    pool: Arc<T>,
    events: EventSender,
    broker: Arc<dyn Broker>,
    provisioner: Arc<ProvisionerRegistry>,
    auth: AuthConfig,
    rate_limit: RateLimitConfig,
) -> Router {
    let auth_config_arc = Arc::new(auth);
    let rate_limiter = Arc::new(RateLimiter::new(rate_limit));
    let cors = CorsLayer::new()
        .allow_origin(Any)
        .allow_methods(Any)
        .allow_headers(Any)
        .expose_headers(Any);

    Router::new()
        .route("/health", get(health))
        .route("/metrics", get(metrics))
        .route("/ready", get(ready::<T>).layer(Extension(pool.clone())))
        .route("/openapi.json", get(crate::openapi::openapi_json))
        .route("/docs", get(crate::openapi::openapi_docs))
        .route("/ws/events", get(ws_events))
        .route(
            "/ws/workflow-runs/{id}",
            get(ws_workflow_run::<T>).layer(Extension(pool.clone())),
        )
        .route(
            "/ws/run-stream/{id}",
            get(ws_run_stream::<T>).layer(Extension(pool.clone())),
        )
        .route(
            "/ws/workflow-node-runs/{id}/stream",
            get(ws_workflow_node_run_stream::<T>).layer(Extension(pool.clone())),
        )
        .route(
            "/ws/desktop-worker",
            get(ws_desktop_worker::<T>).layer(Extension(pool.clone())),
        )
        .route(
            API_WORKFLOWS,
            get(get_workflows::<T>)
                .post(upsert_workflow::<T>)
                .layer(Extension(pool.clone())),
        )
        .route(
            API_WORKFLOWS_VALIDATE,
            post(validate_workflow::<T>).layer(Extension(pool.clone())),
        )
        .route(API_WDL_COMPLETE, post(complete_wdl))
        .route(API_WDL_HOVER, post(hover_wdl))
        .route(
            API_WDL_COMPILE,
            post(compile_wdl::<T>).layer(Extension(pool.clone())),
        )
        .route(
            API_WDL_ANALYZE,
            post(analyze_wdl::<T>).layer(Extension(pool.clone())),
        )
        .route(API_WDL_FORMAT, post(format_wdl))
        .route(API_WDL_DECOMPILE, post(decompile_to_wdl))
        .route(API_WDL_EVALUATE, post(evaluate_expression))
        .route(
            API_WDL_IMPORT,
            post(import_wdl::<T>).layer(Extension(pool.clone())),
        )
        .route(
            API_PACKS_IMPORT,
            post(import_pack::<T>).layer(Extension(pool.clone())),
        )
        .route(
            API_WORKFLOWS_IMPORT,
            post(import_workflow_bundle::<T>).layer(Extension(pool.clone())),
        )
        .route(
            API_WORKFLOWS_EXPORT,
            get(export_workflow_bundle::<T>).layer(Extension(pool.clone())),
        )
        .route(
            "/workflows/{id}",
            get(get_workflow::<T>)
                .patch(upsert_workflow::<T>)
                .delete(delete_workflow::<T>)
                .layer(Extension(pool.clone())),
        )
        .route(
            "/workflows/{id}/export",
            get(export_single_workflow_bundle::<T>).layer(Extension(pool.clone())),
        )
        .route(
            "/workflows/{id}/duplicate",
            post(duplicate_workflow::<T>).layer(Extension(pool.clone())),
        )
        .route(
            "/workflows/{id}/owner",
            axum::routing::patch(set_workflow_owner::<T>).layer(Extension(pool.clone())),
        )
        .route(
            "/workflows/{id}/triggers",
            get(get_workflow_triggers::<T>)
                .post(upsert_workflow_trigger::<T>)
                .layer(Extension(pool.clone())),
        )
        .route(
            API_WORKFLOW_TRIGGERS_DUE,
            get(get_due_workflow_triggers::<T>).layer(Extension(pool.clone())),
        )
        .route(
            API_SCHEDULER_WORKFLOW_TRIGGER_FIRINGS_CLAIM,
            post(claim_due_workflow_trigger_firings::<T>).layer(Extension(pool.clone())),
        )
        .route(
            "/workflow_triggers/{id}",
            get(get_workflow_trigger::<T>)
                .patch(update_workflow_trigger::<T>)
                .delete(delete_workflow_trigger::<T>)
                .layer(Extension(pool.clone())),
        )
        .route(
            "/workflow_triggers/{id}/runs",
            post(create_workflow_trigger_run::<T>).layer(Extension(pool.clone())),
        )
        .route(
            API_PIPELINES,
            get(get_pipelines::<T>)
                .post(create_pipeline::<T>)
                .layer(Extension(pool.clone())),
        )
        .route(
            "/pipelines/{id}",
            get(get_pipeline::<T>)
                .patch(update_pipeline::<T>)
                .delete(delete_pipeline::<T>)
                .layer(Extension(pool.clone())),
        )
        .route(
            "/pipelines/{id}/owner",
            patch(set_pipeline_owner::<T>).layer(Extension(pool.clone())),
        )
        .route(
            API_WORKFLOW_RUNS,
            get(get_workflow_runs::<T>).layer(Extension(pool.clone())),
        )
        .route(
            API_REPLICAS,
            get(get_replicas::<T>).layer(Extension(pool.clone())),
        )
        .route(
            "/replicas/register",
            post(register_replica::<T>).layer(Extension(pool.clone())),
        )
        .route(
            "/replicas/{replica_id}/heartbeat",
            post(heartbeat_replica::<T>).layer(Extension(pool.clone())),
        )
        .route(
            "/replicas/{replica_id}/offline",
            post(mark_replica_offline::<T>).layer(Extension(pool.clone())),
        )
        .route(
            "/replicas/{replica_id}/providers",
            get(get_replica_providers::<T>)
                .post(upsert_replica_provider::<T>)
                .layer(Extension(pool.clone())),
        )
        .route(
            "/replicas/{replica_id}/samples",
            get(get_replica_samples::<T>).layer(Extension(pool.clone())),
        )
        .route("/nodes/backends", get(get_node_backends))
        .route("/nodes", get(get_nodes))
        .route("/nodes/scale", post(scale_nodes))
        .route("/nodes/stop", post(stop_node))
        .route(
            API_SCHEDULER_WORKFLOW_RUNS_CLAIM,
            post(claim_workflow_runs_for_scheduler::<T>).layer(Extension(pool.clone())),
        )
        .route(
            API_SCHEDULER_READY_NODES_CLAIM,
            post(claim_ready_nodes::<T>).layer(Extension(pool.clone())),
        )
        .route(API_RUNS, get(get_runs::<T>).layer(Extension(pool.clone())))
        .route(
            "/runs/{id}",
            patch(update_run::<T>).layer(Extension(pool.clone())),
        )
        .route(
            "/runs/{id}/chunks",
            get(get_run_chunks::<T>)
                .post(append_run_chunk::<T>)
                .layer(Extension(pool.clone())),
        )
        .route(
            "/runs/{id}/artifacts",
            get(get_run_artifacts::<T>)
                .post(add_run_artifact::<T>)
                .layer(Extension(pool.clone())),
        )
        .route(
            API_ARTIFACTS,
            get(list_artifacts::<T>).layer(Extension(pool.clone())),
        )
        .route(
            "/artifacts/upload",
            post(upload_artifact::<T>).layer(Extension(pool.clone())),
        )
        .route(
            "/artifacts/{id}/download",
            get(download_artifact::<T>).layer(Extension(pool.clone())),
        )
        .route(
            "/artifacts/{id}",
            axum::routing::delete(delete_artifact::<T>).layer(Extension(pool.clone())),
        )
        .route(
            "/notifications",
            get(list_notifications::<T>)
                .post(create_notification::<T>)
                .layer(Extension(pool.clone())),
        )
        .route(
            "/notifications/{id}",
            axum::routing::delete(delete_notification::<T>).layer(Extension(pool.clone())),
        )
        .route(
            "/notifications/{id}/mark_read",
            post(mark_notification_read::<T>).layer(Extension(pool.clone())),
        )
        .route(
            "/notifications/mark_all_read",
            post(mark_all_notifications_read::<T>).layer(Extension(pool.clone())),
        )
        .route(
            "/workflows/{id}/runs",
            post(create_workflow_run::<T>).layer(Extension(pool.clone())),
        )
        .route(
            "/workflow_runs/{id}",
            get(get_workflow_run::<T>)
                .patch(update_workflow_run::<T>)
                .layer(Extension(pool.clone())),
        )
        .route(
            "/scheduler/workflow_runs/{id}/claim/renew",
            post(renew_workflow_run_claim::<T>).layer(Extension(pool.clone())),
        )
        .route(
            "/scheduler/workflow_runs/{id}/claim/release",
            post(release_workflow_run_claim::<T>).layer(Extension(pool.clone())),
        )
        .route(
            API_SCHEDULER_ACTION_DISPATCHES,
            post(enqueue_action_dispatch::<T>).layer(Extension(pool.clone())),
        )
        .route(
            API_SCHEDULER_ACTION_DISPATCHES_PENDING,
            get(pending_action_dispatches::<T>).layer(Extension(pool.clone())),
        )
        .route(
            API_SCHEDULER_ACTION_DISPATCHES_CLAIM,
            post(claim_action_dispatches::<T>).layer(Extension(pool.clone())),
        )
        .route(
            "/scheduler/action_dispatches/{id}/published",
            post(mark_action_dispatch_published::<T>).layer(Extension(pool.clone())),
        )
        .route(
            "/scheduler/action_dispatches/{id}/failed",
            post(mark_action_dispatch_failed::<T>).layer(Extension(pool.clone())),
        )
        .route(
            "/scheduler/ready_nodes/{id}/process",
            post(process_ready_node::<T>).layer(Extension(pool.clone())),
        )
        .route(
            "/workflow_runs/{id}/debug/command",
            post(debug_command::<T>).layer(Extension(pool.clone())),
        )
        .route(
            "/workflow_runs/{id}/debug/step",
            post(step_debug_workflow_run::<T>).layer(Extension(pool.clone())),
        )
        .route(
            "/workflow_runs/{id}/debug/continue",
            post(continue_debug_workflow_run::<T>).layer(Extension(pool.clone())),
        )
        .route(
            "/workflow_runs/{id}/debug",
            patch(update_workflow_run_debug::<T>).layer(Extension(pool.clone())),
        )
        .route(
            "/workflow_runs/{id}/cancel",
            post(cancel_workflow_run::<T>).layer(Extension(pool.clone())),
        )
        .route(
            "/workflow_runs/{id}/pause",
            post(pause_workflow_run::<T>).layer(Extension(pool.clone())),
        )
        .route(
            "/workflow_runs/{id}/resume",
            post(resume_workflow_run::<T>).layer(Extension(pool.clone())),
        )
        .route(
            "/workflow_runs/{id}/signals",
            post(deliver_signal::<T>).layer(Extension(pool.clone())),
        )
        .route(
            "/workflow_runs/{id}/debug/run_to_cursor",
            post(run_to_cursor_workflow_run::<T>).layer(Extension(pool.clone())),
        )
        .route(
            "/workflow_runs/{id}/debug/skip",
            post(skip_debug_workflow_node::<T>).layer(Extension(pool.clone())),
        )
        .route(
            "/workflow_runs/{id}/debug/rerun_node",
            post(rerun_debug_workflow_node::<T>).layer(Extension(pool.clone())),
        )
        .route(
            "/workflow_runs/{id}/replay",
            post(replay_workflow_run::<T>).layer(Extension(pool.clone())),
        )
        .route(
            "/workflow_runs/{id}/rename",
            post(rename_workflow_run::<T>).layer(Extension(pool.clone())),
        )
        .route("/supervisor/status", get(get_supervisor_status))
        .route(
            "/workflow_runs/{id}/nodes",
            post(create_workflow_node_run::<T>).layer(Extension(pool.clone())),
        )
        .route(
            "/workflow_node_runs/{id}/claim",
            post(claim_workflow_node_run_executor::<T>).layer(Extension(pool.clone())),
        )
        .route(
            "/workflow_node_runs/{id}/release",
            post(release_workflow_node_run_executor::<T>).layer(Extension(pool.clone())),
        )
        .route(
            "/workflow_node_runs/{id}",
            patch(update_workflow_node_run::<T>).layer(Extension(pool.clone())),
        )
        .route(
            "/workflow_node_runs/{id}/input",
            post(resolve_workflow_input::<T>).layer(Extension(pool.clone())),
        )
        .route(
            "/workflow_node_runs/{id}/chunks",
            get(get_workflow_node_run_chunks::<T>)
                .post(append_workflow_node_run_chunk::<T>)
                .layer(Extension(pool.clone())),
        )
        .route(
            "/workflow_node_runs/{id}/artifacts",
            get(get_workflow_node_run_artifacts::<T>)
                .post(add_workflow_node_run_artifact::<T>)
                .layer(Extension(pool.clone())),
        )
        .route(
            "/workflow_runs/{id}/artifacts",
            get(get_workflow_run_artifacts::<T>).layer(Extension(pool.clone())),
        )
        .route(
            "/catalog/items",
            get(get_catalog_items::<T>)
                .post(upsert_catalog_item::<T>)
                .layer(Extension(pool.clone())),
        )
        .route(
            "/external_items",
            get(get_external_items::<T>)
                .post(create_external_item::<T>)
                .layer(Extension(pool.clone())),
        )
        .route(
            "/gates",
            get(get_gates::<T>)
                .post(create_gate::<T>)
                .layer(Extension(pool.clone())),
        )
        .route(
            "/gates/{id}",
            get(get_gate::<T>)
                .delete(delete_gate::<T>)
                .layer(Extension(pool.clone())),
        )
        .route(
            "/gates/{id}/open",
            post(open_gate::<T>).layer(Extension(pool.clone())),
        )
        .route(
            "/gates/{id}/close",
            post(close_gate::<T>).layer(Extension(pool.clone())),
        )
        .route(
            "/dead_letters",
            get(get_dead_letters::<T>).layer(Extension(pool.clone())),
        )
        .route(
            "/audit_log",
            get(get_audit_log::<T>).layer(Extension(pool.clone())),
        )
        .route(
            "/automation_events",
            get(get_automation_events::<T>)
                .post(create_automation_event::<T>)
                .layer(Extension(pool.clone())),
        )
        .route(
            "/automation_events/{id}",
            axum::routing::delete(delete_automation_event::<T>).layer(Extension(pool.clone())),
        )
        .route(
            "/approvals",
            get(get_approvals::<T>)
                .post(create_approval::<T>)
                .layer(Extension(pool.clone())),
        )
        .route(
            "/approvals/{id}/approve",
            post(approve_request::<T>).layer(Extension(pool.clone())),
        )
        .route(
            "/approvals/{id}/reject",
            post(reject_request::<T>).layer(Extension(pool.clone())),
        )
        .route(
            "/idempotency_keys",
            get(get_idempotency_key::<T>)
                .post(put_idempotency_key::<T>)
                .layer(Extension(pool.clone())),
        )
        .route(
            "/credentials",
            get(get_credential::<T>)
                .post(put_credential::<T>)
                .delete(delete_credential::<T>)
                .layer(Extension(pool.clone())),
        )
        .route(
            "/credentials/import",
            post(import_secret_bundle::<T>).layer(Extension(pool.clone())),
        )
        .route(
            "/credentials/reencrypt",
            post(reencrypt_settings::<T>).layer(Extension(pool.clone())),
        )
        .route(
            API_PROVIDERS,
            get(get_providers::<T>)
                .post(upsert_provider::<T>)
                .layer(Extension(pool.clone())),
        )
        .route(
            "/providers/import",
            post(import_provider_bundle::<T>).layer(Extension(pool.clone())),
        )
        .route("/node-kinds", get(get_node_kinds))
        .route("/trigger-kinds", get(get_trigger_kinds))
        .route("/catalog/enums", get(get_enum_catalogs))
        .route(
            "/webhooks/wake",
            post(webhook_wake::<T>).layer(Extension(pool.clone())),
        )
        .route(
            "/webhooks/signal",
            post(webhook_signal::<T>).layer(Extension(pool.clone())),
        )
        .route("/auth/config", get(auth_config))
        .route(
            "/auth/login",
            post(login::<T>).layer(Extension(pool.clone())),
        )
        .route(
            "/auth/refresh",
            post(refresh::<T>).layer(Extension(pool.clone())),
        )
        .route(
            "/auth/logout",
            post(logout::<T>).layer(Extension(pool.clone())),
        )
        .route("/auth/me", get(me::<T>).layer(Extension(pool.clone())))
        .route(
            "/users",
            get(list_users::<T>)
                .post(create_user::<T>)
                .layer(Extension(pool.clone())),
        )
        .route(
            "/users/{id}",
            patch(update_user::<T>)
                .delete(delete_user::<T>)
                .layer(Extension(pool.clone())),
        )
        .route(
            "/users/{id}/teams",
            get(list_user_teams::<T>).layer(Extension(pool.clone())),
        )
        .route(
            "/api_keys",
            get(list_api_keys::<T>)
                .post(create_api_key::<T>)
                .layer(Extension(pool.clone())),
        )
        .route(
            "/api_keys/{id}",
            patch(update_api_key::<T>)
                .delete(revoke_api_key::<T>)
                .layer(Extension(pool.clone())),
        )
        .route(
            "/api_keys/{id}/rotate",
            post(rotate_api_key::<T>).layer(Extension(pool.clone())),
        )
        .route(
            "/workflows/{id}/grants",
            get(list_workflow_grants::<T>)
                .post(create_workflow_grant::<T>)
                .layer(Extension(pool.clone())),
        )
        .route(
            "/workflows/{id}/grants/{grant_id}",
            delete(revoke_workflow_grant::<T>).layer(Extension(pool.clone())),
        )
        .route(
            "/teams",
            get(list_teams::<T>)
                .post(create_team::<T>)
                .layer(Extension(pool.clone())),
        )
        .route(
            "/teams/{id}",
            patch(update_team::<T>)
                .delete(delete_team::<T>)
                .layer(Extension(pool.clone())),
        )
        .route(
            "/teams/{id}/members",
            get(list_team_members::<T>)
                .post(add_team_member::<T>)
                .layer(Extension(pool.clone())),
        )
        .route(
            "/teams/{id}/members/{user_id}",
            delete(remove_team_member::<T>).layer(Extension(pool.clone())),
        )
        .route(
            "/auth/switch-org",
            post(switch_org::<T>).layer(Extension(pool.clone())),
        )
        .route(
            "/orgs",
            get(list_orgs::<T>)
                .post(create_org::<T>)
                .layer(Extension(pool.clone())),
        )
        .route(
            "/orgs/me",
            get(list_my_orgs::<T>).layer(Extension(pool.clone())),
        )
        .route(
            "/orgs/{id}",
            get(get_org::<T>)
                .patch(update_org::<T>)
                .delete(delete_org::<T>)
                .layer(Extension(pool.clone())),
        )
        .route(
            "/orgs/{id}/members",
            get(list_org_members::<T>)
                .post(add_org_member::<T>)
                .layer(Extension(pool.clone())),
        )
        .route(
            "/orgs/{id}/members/{user_id}",
            patch(update_org_member::<T>)
                .delete(remove_org_member::<T>)
                .layer(Extension(pool.clone())),
        )
        .route("/rate-card", get(get_rate_card))
        .route(
            "/orgs/{id}/nodes",
            get(get_org_nodes::<T>).layer(Extension(pool.clone())),
        )
        .route(
            "/orgs/{id}/nodes/scale",
            post(scale_org_nodes::<T>).layer(Extension(pool.clone())),
        )
        .route(
            "/orgs/{id}/quota",
            get(get_org_quota::<T>)
                .put(put_org_quota::<T>)
                .layer(Extension(pool.clone())),
        )
        .route(
            "/orgs/{id}/usage",
            get(get_org_usage::<T>).layer(Extension(pool.clone())),
        )
        .layer(Extension(events))
        .layer(Extension(broker))
        .layer(Extension(provisioner))
        .layer(Extension(auth_config_arc.clone()))
        // the rate limiter is layered inside the auth middleware so it can key by the resolved
        // principal; auth inserts the `AuthContext` before this layer runs.
        .layer(from_fn_with_state(rate_limiter, rate_limit_middleware))
        .layer(from_fn_with_state(
            AuthState {
                config: auth_config_arc,
                db: pool.clone(),
            },
            auth_middleware::<T>,
        ))
        .layer(cors)
        // cap request bodies for every route. layered here (after all routes are added) so axum
        // actually applies it; placed before `Router::new()` had any routes, it wrapped nothing and
        // requests silently fell back to axum's stricter 2 MB default. 10 MB accommodates pack uploads.
        .layer(DefaultBodyLimit::max(10 * 1024 * 1024))
        // recover from any panic in a handler or inner middleware so a single bad request returns a
        // 500 instead of dropping the connection or poisoning the runtime.
        .layer(CatchPanicLayer::custom(handle_panic))
        // outermost layer: open a request span parented to any inbound w3c trace context so logs and
        // otel spans for this request continue the caller's distributed trace.
        .layer(axum::middleware::from_fn(trace_propagation_middleware))
}

const REQUEST_ID_HEADER: &str = "x-request-id";

/// open a per-request tracing span, re-parent it onto any inbound `traceparent` header so the server
/// side of a distributed trace links to the caller, and log an access line with status/duration once
/// the handler completes. a no-op for trace context when otel is off, leaving an ordinary local span;
/// the request id still works without otel, since it is generated locally rather than derived from a
/// trace context.
async fn trace_propagation_middleware(
    request: axum::extract::Request,
    next: axum::middleware::Next,
) -> axum::response::Response {
    use tracing::Instrument;

    let method = request.method().clone();
    let path = request.uri().path().to_string();
    // reuse an inbound request id from a fronting proxy/gateway when present, so this request's logs
    // line up with that layer's; otherwise mint one so every request is correlatable even with otel off.
    let request_id = request
        .headers()
        .get(REQUEST_ID_HEADER)
        .and_then(|value| value.to_str().ok())
        .map(str::to_string)
        .unwrap_or_else(|| uuid::Uuid::now_v7().to_string());

    let span = tracing::info_span!(
        "http_request",
        method = %method,
        path = %path,
        request_id = %request_id,
    );
    runinator_utilities::telemetry::apply_http_context(&span, request.headers());

    async move {
        let started = std::time::Instant::now();
        let mut response = next.run(request).await;
        let duration_ms = started.elapsed().as_millis() as u64;
        let status = response.status().as_u16();
        if status >= 500 {
            tracing::error!(status, duration_ms, "request completed");
        } else if status >= 400 {
            tracing::warn!(status, duration_ms, "request completed");
        } else {
            tracing::info!(status, duration_ms, "request completed");
        }
        if let Ok(value) = axum::http::HeaderValue::from_str(&request_id) {
            response.headers_mut().insert(REQUEST_ID_HEADER, value);
        }
        response
    }
    .instrument(span)
    .await
}

/// turn a recovered handler panic into the standard json error envelope. the panic payload is logged
/// in full; the client gets a generic message so internal details are not leaked.
pub(crate) fn handle_panic(
    panic: Box<dyn std::any::Any + Send + 'static>,
) -> axum::response::Response {
    let detail = panic
        .downcast_ref::<&str>()
        .map(|s| s.to_string())
        .or_else(|| panic.downcast_ref::<String>().cloned())
        .unwrap_or_else(|| "unknown panic payload".to_string());
    log::error!("recovered from panic in HTTP handler: {detail}");
    crate::stability::record_handler_panic();
    (
        axum::http::StatusCode::INTERNAL_SERVER_ERROR,
        axum::Json(ApiResponse::ApiError(ApiError::new(
            "internal server error",
        ))),
    )
        .into_response()
}

#[cfg(test)]
#[path = "router_tests.rs"]
mod tests;
