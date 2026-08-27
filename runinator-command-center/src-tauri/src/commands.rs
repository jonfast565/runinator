use runinator_models::{
    api_routes::API_WORKFLOWS_SIMULATE,
    orchestration::{NodeTransition, NodeTransitionStat},
    pipelines::{Pipeline, PipelineMemberAttempt, PipelineRun, PipelineRunDetail, PipelineTrigger},
    providers::ProviderMetadata,
    replicas::ReplicaListResponse,
    web::TaskResponse,
    workflow_vm::{
        WorkflowContinuation, WorkflowEffect, WorkflowEffectOutputEvent, WorkflowEffectStatus,
        WorkflowJournalRecord, WorkflowVmCursor,
    },
    workflows::{
        WorkflowBundle, WorkflowDefinition, WorkflowRun, WorkflowSimulateRequest, WorkflowTrigger,
    },
};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use tauri::{AppHandle, State};
use uuid::Uuid;

use crate::{
    client::{
        build_state_url, delete, get_json, handle_response, patch_json, post_bytes, post_empty,
        post_json, put_json,
    },
    discovery::start_discovery_thread,
    error::{CommandError, CommandResult},
    state::CommandCenterState,
    types::{
        CredentialPutRequest, CredentialSummary, DiagnosticSummary, ServiceStatus,
        WorkflowRunCreated, WorkflowRunDetail,
    },
};
use runinator_rexrap::{CompileOptions, Severity};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkflowRexRapSaveRequest {
    pub source: String,
    pub enabled: bool,
    #[serde(default)]
    pub workflow_id: Option<Uuid>,
    #[serde(default)]
    pub triggers: Vec<WorkflowTrigger>,
    #[serde(default)]
    pub ui: Option<Value>,
}

#[tauri::command]
pub async fn get_service_status(
    state: State<'_, CommandCenterState>,
) -> CommandResult<ServiceStatus> {
    Ok(ServiceStatus {
        service_url: state.service_url.read().await.clone(),
    })
}

#[tauri::command]
pub fn start_service_discovery(app: AppHandle, state: State<'_, CommandCenterState>) {
    start_discovery_thread(app, state.inner().clone());
}

// ---- auth ----

/// append a `?`-joined query string to `base` when any params are present.
fn with_query(base: &str, params: &[String]) -> String {
    if params.is_empty() {
        base.to_string()
    } else {
        format!("{base}?{}", params.join("&"))
    }
}

mod identity;
pub use identity::*;
mod workflows;
pub use workflows::*;
mod pipelines;
pub use pipelines::*;
mod orchestrations;
pub use orchestrations::*;
mod runs;
pub use runs::*;
mod catalog;
pub use catalog::*;
mod organizations;
pub use organizations::*;
mod interactions;
pub use interactions::*;
mod functions;
pub use functions::*;
