//! sqlite-backed persistence tests, partitioned by the store role each one exercises.
//!
//! these run against a real sqlite file rather than a fake: what is under test is the sql — leases,
//! cascades, dedupe, and transaction boundaries — none of which survive being mocked. shared setup
//! lives here so each submodule picks it up through its `use super` glob.

mod archive;
mod audit;
mod auth;
mod definitions;
mod dispatch;
mod leases;
mod parity;
mod ready_nodes;
mod results;
mod revisions;
mod runs;
mod schedules;
mod settings;
mod transitions;

use super::*;
use crate::archive::ArchiveTable;
use chrono::{Duration, Utc};
use runinator_comm::{ActionCommand, WorkflowResultEvent};
use runinator_models::value::Value;
use runinator_models::{
    auth::{ApiKey, ApiKeyRecord, Grant, Permission, PrincipalType, ResourceType},
    notifications::NewNotification,
    orchestration::IdempotencyClaim,
    orgs::OrgRole,
    revisions::{RevisionSource, WorkflowRevision},
    runs::NewRunChunk,
    schedules::{BackfillRequest, NewFreezeWindow},
    settings::SettingKind,
    workflows::{
        WorkflowAction, WorkflowDefinition, WorkflowGraph, WorkflowNodeRun, WorkflowStatus,
        WorkflowTrigger, WorkflowTriggerKind,
    },
};
use runinator_store::prelude::*;
use uuid::Uuid;

fn workflow(name: &str) -> WorkflowDefinition {
    WorkflowDefinition {
        id: None,
        name: name.to_string(),
        namespace: None,
        org_id: None,
        version: runinator_models::semver::SemVer::new(1, 0, 0),
        enabled: true,
        input_type: runinator_models::types::RuninatorType::Any,
        definition: WorkflowGraph::from_value(runinator_models::json!({ "nodes": [] })).unwrap(),
        created_at: None,
        updated_at: None,
    }
}

async fn create_node_run(db: &SqliteDb) -> WorkflowNodeRun {
    let workflow_id = db
        .upsert_workflow(&workflow("result-test"))
        .await
        .unwrap()
        .id
        .unwrap();
    let snapshot = db.fetch_workflow(workflow_id).await.unwrap().unwrap();
    let workflow_run = db
        .create_workflow_run(
            workflow_id,
            snapshot,
            runinator_models::json!({}),
            runinator_models::json!({}),
            None,
            Default::default(),
        )
        .await
        .unwrap();
    db.create_workflow_node_run(
        workflow_run.id,
        "node-a".into(),
        runinator_models::json!({}),
        None,
            cursor,)
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
        },
        attempt: 1,
        parameters: runinator_models::json!({}),
        target: Default::default(),
        trace_id: Uuid::nil(),
        trace_context: Default::default(),
        notification_delivery_id: None,
        idempotency_key: None,
    }
}
