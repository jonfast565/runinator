use std::sync::Arc;

use axum::{
    Extension, Router,
    routing::{get, patch, post},
};
use runinator_database::interfaces::DatabaseImpl;

use crate::events::EventSender;
use crate::handlers::{
    artifacts::{
        add_run_artifact, download_artifact, get_run_artifacts, list_artifacts, upload_artifact,
    },
    automation::{
        approve_request, create_approval, create_automation_event, create_change_set,
        create_external_item, create_external_resource, create_feedback, create_gate,
        create_workspace, get_approvals, get_automation_events, get_change_sets,
        get_external_items, get_external_resources, get_feedback, get_gates, get_idempotency_key,
        get_workspaces, put_idempotency_key, reject_request,
    },
    catalog::{get_catalog_items, upsert_catalog_item},
    credentials::{delete_credential, get_credential, import_secret_bundle, put_credential},
    debug::{
        continue_debug_workflow_run, rerun_debug_workflow_node, run_to_cursor_workflow_run,
        skip_debug_workflow_node, step_debug_workflow_run, update_workflow_run_debug,
    },
    node_runs::{
        add_workflow_node_run_artifact, append_workflow_node_run_chunk, create_workflow_node_run,
        get_workflow_node_run_artifacts, get_workflow_node_run_chunks, update_workflow_node_run,
    },
    notifications::{
        create_notification, list_notifications, mark_all_notifications_read,
        mark_notification_read,
    },
    providers::{get_providers, import_provider_bundle, upsert_provider},
    runs::{
        append_run_chunk, cancel_workflow_run, create_workflow_run, create_workflow_trigger_run,
        get_run_chunks, get_runs, get_workflow_run, get_workflow_runs, rename_workflow_run,
        replay_workflow_run, update_run, update_workflow_run,
    },
    supervisor::get_supervisor_status,
    triggers::{
        delete_workflow_trigger, get_due_workflow_triggers, get_workflow_trigger,
        get_workflow_triggers, update_workflow_trigger, upsert_workflow_trigger,
    },
    webhook::webhook_wake,
    workflows::{
        delete_workflow, export_single_workflow_bundle, export_workflow_bundle, get_workflow,
        get_workflows, import_workflow_bundle, upsert_workflow, validate_workflow,
    },
};
use crate::websocket::{ws_events, ws_run_stream, ws_workflow_node_run_stream, ws_workflow_run};

pub fn build_router<T: DatabaseImpl>(pool: Arc<T>, events: EventSender) -> Router {
    Router::new()
        .route("/ws/events", get(ws_events))
        .route("/ws/workflow-runs/{id}", get(ws_workflow_run::<T>))
        .route("/ws/run-stream/{id}", get(ws_run_stream::<T>))
        .route(
            "/ws/workflow-node-runs/{id}/stream",
            get(ws_workflow_node_run_stream::<T>),
        )
        .route(
            "/workflows",
            get(get_workflows::<T>)
                .post(upsert_workflow::<T>)
                .layer(Extension(pool.clone())),
        )
        .route("/workflows/validate", post(validate_workflow))
        .route(
            "/workflows/import",
            post(import_workflow_bundle::<T>).layer(Extension(pool.clone())),
        )
        .route(
            "/workflows/export",
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
            "/workflows/{id}/triggers",
            get(get_workflow_triggers::<T>)
                .post(upsert_workflow_trigger::<T>)
                .layer(Extension(pool.clone())),
        )
        .route(
            "/workflow_triggers/due",
            get(get_due_workflow_triggers::<T>).layer(Extension(pool.clone())),
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
            "/workflow_runs",
            get(get_workflow_runs::<T>).layer(Extension(pool.clone())),
        )
        .route("/runs", get(get_runs::<T>).layer(Extension(pool.clone())))
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
            "/artifacts",
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
            "/notifications",
            get(list_notifications::<T>)
                .post(create_notification::<T>)
                .layer(Extension(pool.clone())),
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
            "/workflow_node_runs/{id}",
            patch(update_workflow_node_run::<T>).layer(Extension(pool.clone())),
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
            "/external_resources",
            get(get_external_resources::<T>)
                .post(create_external_resource::<T>)
                .layer(Extension(pool.clone())),
        )
        .route(
            "/feedback",
            get(get_feedback::<T>)
                .post(create_feedback::<T>)
                .layer(Extension(pool.clone())),
        )
        .route(
            "/gates",
            get(get_gates::<T>)
                .post(create_gate::<T>)
                .layer(Extension(pool.clone())),
        )
        .route(
            "/workspaces",
            get(get_workspaces::<T>)
                .post(create_workspace::<T>)
                .layer(Extension(pool.clone())),
        )
        .route(
            "/change_sets",
            get(get_change_sets::<T>)
                .post(create_change_set::<T>)
                .layer(Extension(pool.clone())),
        )
        .route(
            "/automation_events",
            get(get_automation_events::<T>)
                .post(create_automation_event::<T>)
                .layer(Extension(pool.clone())),
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
            get(get_credential)
                .post(put_credential)
                .delete(delete_credential),
        )
        .route("/credentials/import", post(import_secret_bundle))
        .route(
            "/providers",
            get(get_providers::<T>)
                .post(upsert_provider::<T>)
                .layer(Extension(pool.clone())),
        )
        .route(
            "/providers/import",
            post(import_provider_bundle::<T>).layer(Extension(pool.clone())),
        )
        .route(
            "/webhooks/wake",
            post(webhook_wake::<T>).layer(Extension(pool.clone())),
        )
        .layer(Extension(events))
}
