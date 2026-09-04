//! sqlite-backed persistence tests, partitioned by the store role each one exercises.
//!
//! these run against a real sqlite file rather than a fake: what is under test is the sql — leases,
//! cascades, dedupe, and transaction boundaries — none of which survive being mocked. shared setup
//! lives here so each submodule picks it up through its `use super` glob.

mod archive;
mod audit;
mod auth;
mod definitions;
mod execution_profiles;
mod implicit_platform;
mod ingress_control;
mod notifications;
mod orchestrations;
mod pack_transaction;
mod parity;
mod revisions;
mod schema;
mod settings;
mod workflow_vm;
mod workspaces;

use super::*;
use chrono::{Duration, Utc};
use runinator_models::value::Value;
use runinator_models::{
    auth::{ApiKey, ApiKeyRecord, Grant, Permission, PrincipalKind, PrincipalType, ResourceType},
    notifications::NewNotification,
    orgs::OrgRole,
    rbac::{PlatformRole, ResourceOwnership, Role, ScopeRef, TeamRole},
    revisions::{RevisionSource, WorkflowRevision},
    settings::SettingKind,
    workflows::{WorkflowDefinition, WorkflowGraph, WorkflowStatus},
};
use runinator_store::archive::ArchiveTable;
use runinator_store::prelude::*;
use uuid::Uuid;

fn workflow(name: &str) -> WorkflowDefinition {
    WorkflowDefinition {
        id: None,
        name: name.to_string(),
        key: None,
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
