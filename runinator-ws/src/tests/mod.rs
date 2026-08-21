//! the web service’s behaviour tests, partitioned by subject.
//!
//! most of these boot a real `SqliteDb` and drive the engine through it, because what they cover is
//! the seam between an http handler, the reducer, and persistence — the part no single crate owns.
//! prefer `runinator-runtime`’s fake-backed suite for anything that is purely a node-handler
//! decision; reach for this layer when the database or the broker is part of the assertion.
//!
//! shared fixtures live here so each submodule picks them up through its `use super` glob.

mod authz;
mod bootstrap;
mod functions;
mod models;
mod orgs;
mod packs;
mod revisions;
mod rexrap;
mod runs;
mod users;
mod validation;

use std::sync::Arc;

use axum::{Extension, Json, extract::Path, http::StatusCode};
use runinator_broker::in_memory::InMemoryBroker;
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
    workflows::{
        WorkflowBundle, WorkflowDefinition, WorkflowGraph, WorkflowStatus, WorkflowTrigger,
        WorkflowTriggerKind,
    },
};
use runinator_rexrap::RexRapFragmentKind;
use runinator_workflows::{WorkflowTypeDiagnostic, WorkflowValidationError};
use runinator_ws_middleware::authz::{AuthContextExt, AuthzChecker};
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
        session_id: None,
        platform_role: None,
        assignments: Vec::new(),
        system_role: None,
        action_ceiling: Vec::new(),
        kind: PrincipalKind::User,
        org_id: None,
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

/// Drive the durable VM the same way the engine loop does in production.  Pipeline graph
/// advancement deliberately lives above the VM host, so terminal member runs must be handed back
/// to the engine repository before the next downstream member is created.
async fn drain_ready_nodes(db: &SqliteDb) {
    let host = runinator_runtime::WorkflowVmHost::new(db);
    for _ in 0..16 {
        let outcomes = host.drive_runnable("test-vm".into(), 100).await.unwrap();
        if outcomes.is_empty() {
            break;
        }
        for outcome in outcomes {
            let settled_run_id = match outcome {
                runinator_runtime::WorkflowVmDriveOutcome::Completed { settled_run_id }
                | runinator_runtime::WorkflowVmDriveOutcome::Failed { settled_run_id } => {
                    settled_run_id
                }
                _ => None,
            };
            if let Some(run_id) = settled_run_id {
                crate::repository::advance_pipeline_from_vm_terminal(db, run_id)
                    .await
                    .unwrap();
            }
        }
    }
}
