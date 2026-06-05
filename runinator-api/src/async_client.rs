use chrono::{DateTime, Utc};
use reqwest::{Client, Response, Url};
use runinator_comm::{ActionCommand, ActionDispatchRecord};
use runinator_models::json;
use runinator_models::value::Value;
use runinator_models::{
    api_routes::{
        api_approval_command, api_run, api_run_artifacts, api_run_chunks,
        api_scheduler_action_dispatch_failed, api_scheduler_action_dispatch_published,
        api_scheduler_ready_node_process, api_scheduler_workflow_run_claim_release,
        api_scheduler_workflow_run_claim_renew, api_workflow, api_workflow_node_run,
        api_workflow_node_run_artifacts, api_workflow_node_run_chunks, api_workflow_run,
        api_workflow_run_command, api_workflow_run_nodes, api_workflow_run_rename,
        api_workflow_run_replay, api_workflow_runs, api_workflow_trigger,
        api_workflow_trigger_runs, api_workflow_triggers, API_APPROVALS, API_CREDENTIALS,
        API_IDEMPOTENCY_KEYS, API_PACKS_IMPORT, API_PROVIDERS, API_RUNS,
        API_SCHEDULER_ACTION_DISPATCHES, API_SCHEDULER_ACTION_DISPATCHES_CLAIM,
        API_SCHEDULER_ACTION_DISPATCHES_PENDING, API_SCHEDULER_READY_NODES_CLAIM,
        API_SCHEDULER_WORKFLOW_RUNS_CLAIM, API_SCHEDULER_WORKFLOW_TRIGGER_FIRINGS_CLAIM,
        API_SUPERVISOR_STATUS, API_WORKFLOWS, API_WORKFLOWS_EXPORT, API_WORKFLOWS_IMPORT,
        API_WORKFLOWS_VALIDATE, API_WORKFLOW_RUNS, API_WORKFLOW_TRIGGERS_DUE,
        WORKFLOW_JSON_IMPORT_RISK_ACK, WORKFLOW_JSON_IMPORT_RISK_HEADER,
    },
    bundles::{Bundle, PackImportResult, ProviderBundle, SecretBundle},
    orchestration::ReadyNodeRecord,
    providers::ProviderMetadata,
    runs::{RunStatus, RunSummary},
    settings::{SettingKind, SettingSummary},
    web::TaskResponse,
    workflows::{
        WorkflowBundle, WorkflowDefinition, WorkflowNodeRun, WorkflowNodeRunArtifact,
        WorkflowNodeRunChunk, WorkflowRun, WorkflowStatus, WorkflowTrigger,
    },
};

use crate::{
    error::{ApiError, Result},
    locator::ServiceLocator,
    types::{RunArtifactPayload, RunChunkPayload, RunStatusPayload, WorkflowNodeRunStatusPayload},
};

/// Asynchronous API client that wraps `reqwest::Client` and a service locator.
#[derive(Clone)]
pub struct AsyncApiClient<L> {
    client: Client,
    locator: L,
}

impl<L> AsyncApiClient<L>
where
    L: ServiceLocator,
{
    /// Construct a client with the default `reqwest::Client` configuration.
    pub fn new(locator: L) -> reqwest::Result<Self> {
        let client = Client::builder().build()?;
        Ok(Self { client, locator })
    }

    /// Construct a client using a preconfigured HTTP client instance.
    pub fn with_client(locator: L, client: Client) -> Self {
        Self { client, locator }
    }

    /// Fetch provider/action metadata for task authoring.
    pub async fn fetch_providers(&self) -> Result<Vec<ProviderMetadata>> {
        let url = self.build_url(API_PROVIDERS).await?;
        let response = self.client.get(url.clone()).send().await?;
        let response = Self::handle_response(url, response).await?;
        Ok(response.json::<Vec<ProviderMetadata>>().await?)
    }

    /// Register provider/action metadata with the web service.
    pub async fn upsert_provider(&self, provider: &ProviderMetadata) -> Result<ProviderMetadata> {
        let url = self.build_url(API_PROVIDERS).await?;
        let response = self.client.post(url.clone()).json(provider).send().await?;
        let response = Self::handle_response(url, response).await?;
        Ok(response.json::<ProviderMetadata>().await?)
    }

    pub async fn fetch_run(&self, run_id: i64) -> Result<RunSummary> {
        let url = self.build_url(&api_run(run_id)).await?;
        let response = self.client.get(url.clone()).send().await?;
        let response = Self::handle_response(url, response).await?;
        Ok(response.json::<RunSummary>().await?)
    }

    pub async fn fetch_runs_by_status(&self, status: RunStatus) -> Result<Vec<RunSummary>> {
        let url = self
            .build_url(&format!("{API_RUNS}?status={}", status.as_str()))
            .await?;
        let response = self.client.get(url.clone()).send().await?;
        let response = Self::handle_response(url, response).await?;
        Ok(response.json::<Vec<RunSummary>>().await?)
    }

    pub async fn update_run(
        &self,
        run_id: i64,
        payload: &RunStatusPayload,
    ) -> Result<TaskResponse> {
        let url = self.build_url(&api_run(run_id)).await?;
        let response = self.client.patch(url.clone()).json(payload).send().await?;
        let response = Self::handle_response(url, response).await?;
        Ok(response.json::<TaskResponse>().await?)
    }

    pub async fn append_run_chunk(
        &self,
        run_id: i64,
        payload: &RunChunkPayload,
    ) -> Result<TaskResponse> {
        let url = self.build_url(&api_run_chunks(run_id)).await?;
        let response = self.client.post(url.clone()).json(payload).send().await?;
        let response = Self::handle_response(url, response).await?;
        Ok(response.json::<TaskResponse>().await?)
    }

    pub async fn add_run_artifact(
        &self,
        run_id: i64,
        payload: &RunArtifactPayload,
    ) -> Result<TaskResponse> {
        let url = self.build_url(&api_run_artifacts(run_id)).await?;
        let response = self.client.post(url.clone()).json(payload).send().await?;
        let response = Self::handle_response(url, response).await?;
        Ok(response.json::<TaskResponse>().await?)
    }

    pub async fn fetch_workflow(&self, workflow_id: i64) -> Result<WorkflowDefinition> {
        let url = self.build_url(&api_workflow(workflow_id)).await?;
        let response = self.client.get(url.clone()).send().await?;
        let response = Self::handle_response(url, response).await?;
        Ok(response.json::<WorkflowDefinition>().await?)
    }

    pub async fn fetch_workflows(&self) -> Result<Vec<WorkflowDefinition>> {
        let url = self.build_url(API_WORKFLOWS).await?;
        let response = self.client.get(url.clone()).send().await?;
        let response = Self::handle_response(url, response).await?;
        Ok(response.json::<Vec<WorkflowDefinition>>().await?)
    }

    pub async fn fetch_workflow_by_name(&self, name: &str) -> Result<WorkflowDefinition> {
        let mut url = self.build_url(API_WORKFLOWS).await?;
        url.query_pairs_mut().append_pair("name", name);
        let response = self.client.get(url.clone()).send().await?;
        let response = Self::handle_response(url, response).await?;
        Ok(response.json::<WorkflowDefinition>().await?)
    }

    pub async fn upsert_workflow(
        &self,
        workflow: &WorkflowDefinition,
    ) -> Result<WorkflowDefinition> {
        let url = match workflow.id {
            Some(id) => self.build_url(&api_workflow(id)).await?,
            None => self.build_url(API_WORKFLOWS).await?,
        };
        let response = match workflow.id {
            Some(_) => self.client.patch(url.clone()).json(workflow).send().await?,
            None => self.client.post(url.clone()).json(workflow).send().await?,
        };
        let response = Self::handle_response(url, response).await?;
        Ok(response.json::<WorkflowDefinition>().await?)
    }

    pub async fn validate_workflow(
        &self,
        workflow: &WorkflowDefinition,
    ) -> Result<WorkflowDefinition> {
        let url = self.build_url(API_WORKFLOWS_VALIDATE).await?;
        let response = self.client.post(url.clone()).json(workflow).send().await?;
        let response = Self::handle_response(url, response).await?;
        Ok(response.json::<WorkflowDefinition>().await?)
    }

    /// POST a typed bundle to its associated import endpoint.
    pub async fn import_bundle<B: Bundle>(&self, bundle: &B) -> Result<B> {
        let url = self.build_url(B::RESOURCE).await?;
        let response = self.client.post(url.clone()).json(bundle).send().await?;
        let response = Self::handle_response(url, response).await?;
        Ok(response.json::<B>().await?)
    }

    /// POST a raw JSON workflow bundle after acknowledging that system breakage is possible.
    pub async fn import_workflow_bundle(&self, bundle: &WorkflowBundle) -> Result<WorkflowBundle> {
        let url = self.build_url(API_WORKFLOWS_IMPORT).await?;
        let response = self
            .client
            .post(url.clone())
            .header(
                WORKFLOW_JSON_IMPORT_RISK_HEADER,
                WORKFLOW_JSON_IMPORT_RISK_ACK,
            )
            .json(bundle)
            .send()
            .await?;
        let response = Self::handle_response(url, response).await?;
        Ok(response.json::<WorkflowBundle>().await?)
    }

    /// Build a compiled pack zip (workflows + optional secrets) and POST it to `/packs/import`.
    pub async fn import_pack(
        &self,
        workflows: &WorkflowBundle,
        secrets: Option<&SecretBundle>,
        overwrite: bool,
    ) -> Result<PackImportResult> {
        let body = runinator_utilities::pack::build_pack_zip(workflows, secrets)
            .map_err(|err| ApiError::Pack(err.to_string()))?;
        let mut url = self.build_url(API_PACKS_IMPORT).await?;
        if overwrite {
            url.set_query(Some("overwrite=true"));
        }
        let response = self
            .client
            .post(url.clone())
            .header(reqwest::header::CONTENT_TYPE, "application/zip")
            .body(body)
            .send()
            .await?;
        let response = Self::handle_response(url, response).await?;
        Ok(response.json::<PackImportResult>().await?)
    }

    pub async fn import_provider_bundle(&self, bundle: &ProviderBundle) -> Result<ProviderBundle> {
        self.import_bundle(bundle).await
    }

    pub async fn import_secret_bundle(&self, bundle: &SecretBundle) -> Result<SecretBundle> {
        self.import_bundle(bundle).await
    }

    pub async fn export_workflow_bundle(&self, workflow_id: Option<i64>) -> Result<WorkflowBundle> {
        let path = workflow_id
            .map(|id| format!("{}/export", api_workflow(id)))
            .unwrap_or_else(|| API_WORKFLOWS_EXPORT.into());
        let url = self.build_url(&path).await?;
        let response = self.client.get(url.clone()).send().await?;
        let response = Self::handle_response(url, response).await?;
        Ok(response.json::<WorkflowBundle>().await?)
    }

    pub async fn create_workflow_run(
        &self,
        workflow_id: i64,
        parameters: Value,
    ) -> Result<WorkflowRun> {
        self.create_workflow_run_with_options(workflow_id, parameters, false, None)
            .await
    }

    pub async fn create_named_workflow_run(
        &self,
        workflow_id: i64,
        parameters: Value,
        name: String,
    ) -> Result<WorkflowRun> {
        self.create_workflow_run_with_options(workflow_id, parameters, false, Some(name))
            .await
    }

    pub async fn fetch_workflow_triggers(&self, workflow_id: i64) -> Result<Vec<WorkflowTrigger>> {
        let url = self.build_url(&api_workflow_triggers(workflow_id)).await?;
        let response = self.client.get(url.clone()).send().await?;
        let response = Self::handle_response(url, response).await?;
        Ok(response.json::<Vec<WorkflowTrigger>>().await?)
    }

    pub async fn fetch_due_workflow_triggers(&self) -> Result<Vec<WorkflowTrigger>> {
        let url = self.build_url(API_WORKFLOW_TRIGGERS_DUE).await?;
        let response = self.client.get(url.clone()).send().await?;
        let response = Self::handle_response(url, response).await?;
        Ok(response.json::<Vec<WorkflowTrigger>>().await?)
    }

    pub async fn claim_due_workflow_trigger_firings(
        &self,
        scheduler_id: &str,
        limit: i64,
    ) -> Result<Vec<WorkflowRun>> {
        let url = self
            .build_url(API_SCHEDULER_WORKFLOW_TRIGGER_FIRINGS_CLAIM)
            .await?;
        let response = self
            .client
            .post(url.clone())
            .json(&json!({ "scheduler_id": scheduler_id, "limit": limit }))
            .send()
            .await?;
        let response = Self::handle_response(url, response).await?;
        Ok(response.json::<Vec<WorkflowRun>>().await?)
    }

    pub async fn fetch_workflow_trigger(&self, trigger_id: i64) -> Result<WorkflowTrigger> {
        let url = self.build_url(&api_workflow_trigger(trigger_id)).await?;
        let response = self.client.get(url.clone()).send().await?;
        let response = Self::handle_response(url, response).await?;
        Ok(response.json::<WorkflowTrigger>().await?)
    }

    pub async fn upsert_workflow_trigger(
        &self,
        trigger: &WorkflowTrigger,
    ) -> Result<WorkflowTrigger> {
        let url = match trigger.id {
            Some(id) => self.build_url(&api_workflow_trigger(id)).await?,
            None => {
                self.build_url(&api_workflow_triggers(trigger.workflow_id))
                    .await?
            }
        };
        let response = match trigger.id {
            Some(_) => self.client.patch(url.clone()).json(trigger).send().await?,
            None => self.client.post(url.clone()).json(trigger).send().await?,
        };
        let response = Self::handle_response(url, response).await?;
        Ok(response.json::<WorkflowTrigger>().await?)
    }

    pub async fn delete_workflow_trigger(&self, trigger_id: i64) -> Result<TaskResponse> {
        let url = self.build_url(&api_workflow_trigger(trigger_id)).await?;
        let response = self.client.delete(url.clone()).send().await?;
        let response = Self::handle_response(url, response).await?;
        Ok(response.json::<TaskResponse>().await?)
    }

    pub async fn create_workflow_trigger_run(
        &self,
        trigger_id: i64,
        parameters: Value,
        debug: bool,
    ) -> Result<WorkflowRun> {
        let url = self
            .build_url(&api_workflow_trigger_runs(trigger_id))
            .await?;
        let response = self
            .client
            .post(url.clone())
            .json(&json!({ "parameters": parameters, "debug": debug }))
            .send()
            .await?;
        let response = Self::handle_response(url, response).await?;
        let body = response.json::<Value>().await?;
        serde_json::from_value(
            body.get("run")
                .cloned()
                .ok_or_else(|| ApiError::UnexpectedResponse("missing run".into()))?
                .into(),
        )
        .map_err(|err| ApiError::UnexpectedResponse(err.to_string()))
    }

    pub async fn create_workflow_run_with_debug(
        &self,
        workflow_id: i64,
        parameters: Value,
        debug: bool,
    ) -> Result<WorkflowRun> {
        self.create_workflow_run_with_options(workflow_id, parameters, debug, None)
            .await
    }

    pub async fn create_workflow_run_with_options(
        &self,
        workflow_id: i64,
        parameters: Value,
        debug: bool,
        name: Option<String>,
    ) -> Result<WorkflowRun> {
        let url = self.build_url(&api_workflow_runs(workflow_id)).await?;
        let response = self
            .client
            .post(url.clone())
            .json(&json!({ "parameters": parameters, "debug": debug, "name": name }))
            .send()
            .await?;
        let response = Self::handle_response(url, response).await?;
        let body = response.json::<Value>().await?;
        serde_json::from_value(
            body.get("run")
                .cloned()
                .ok_or_else(|| ApiError::UnexpectedResponse("missing run".into()))?
                .into(),
        )
        .map_err(|err| ApiError::UnexpectedResponse(err.to_string()))
    }

    pub async fn fetch_workflow_runs_by_status(
        &self,
        status: WorkflowStatus,
    ) -> Result<Vec<WorkflowRun>> {
        let url = self
            .build_url(&format!("{API_WORKFLOW_RUNS}?status={}", status.as_str()))
            .await?;
        let response = self.client.get(url.clone()).send().await?;
        let response = Self::handle_response(url, response).await?;
        Ok(response.json::<Vec<WorkflowRun>>().await?)
    }

    pub async fn claim_workflow_runs_for_scheduler(
        &self,
        scheduler_id: &str,
        statuses: &[WorkflowStatus],
        lease_until: DateTime<Utc>,
        limit: i64,
    ) -> Result<Vec<WorkflowRun>> {
        let url = self.build_url(API_SCHEDULER_WORKFLOW_RUNS_CLAIM).await?;
        let response = self
            .client
            .post(url.clone())
            .json(&json!({
                "scheduler_id": scheduler_id,
                "statuses": statuses,
                "lease_until": lease_until,
                "limit": limit
            }))
            .send()
            .await?;
        let response = Self::handle_response(url, response).await?;
        Ok(response.json::<Vec<WorkflowRun>>().await?)
    }

    pub async fn renew_workflow_run_claim(
        &self,
        workflow_run_id: i64,
        scheduler_id: &str,
        lease_until: DateTime<Utc>,
    ) -> Result<TaskResponse> {
        let url = self
            .build_url(&api_scheduler_workflow_run_claim_renew(workflow_run_id))
            .await?;
        let response = self
            .client
            .post(url.clone())
            .json(&json!({ "scheduler_id": scheduler_id, "lease_until": lease_until }))
            .send()
            .await?;
        let response = Self::handle_response(url, response).await?;
        Ok(response.json::<TaskResponse>().await?)
    }

    pub async fn release_workflow_run_claim(
        &self,
        workflow_run_id: i64,
        scheduler_id: &str,
    ) -> Result<TaskResponse> {
        let url = self
            .build_url(&api_scheduler_workflow_run_claim_release(workflow_run_id))
            .await?;
        let response = self
            .client
            .post(url.clone())
            .json(&json!({ "scheduler_id": scheduler_id }))
            .send()
            .await?;
        let response = Self::handle_response(url, response).await?;
        Ok(response.json::<TaskResponse>().await?)
    }

    pub async fn fetch_workflow_runs(
        &self,
        status: Option<WorkflowStatus>,
        workflow_id: Option<i64>,
    ) -> Result<Vec<WorkflowRun>> {
        let mut url = self.build_url(API_WORKFLOW_RUNS).await?;
        if let Some(status) = status {
            url.query_pairs_mut().append_pair("status", status.as_str());
        }
        if let Some(workflow_id) = workflow_id {
            url.query_pairs_mut()
                .append_pair("workflow_id", &workflow_id.to_string());
        }
        let response = self.client.get(url.clone()).send().await?;
        let response = Self::handle_response(url, response).await?;
        Ok(response.json::<Vec<WorkflowRun>>().await?)
    }

    pub async fn fetch_workflow_runs_by_name(
        &self,
        name: &str,
        open_only: bool,
    ) -> Result<Vec<WorkflowRun>> {
        let mut url = self.build_url(API_WORKFLOW_RUNS).await?;
        url.query_pairs_mut()
            .append_pair("name", name)
            .append_pair("open", if open_only { "true" } else { "false" });
        let response = self.client.get(url.clone()).send().await?;
        let response = Self::handle_response(url, response).await?;
        Ok(response.json::<Vec<WorkflowRun>>().await?)
    }

    pub async fn update_workflow_run(
        &self,
        workflow_run_id: i64,
        status: WorkflowStatus,
        active_node_id: Option<String>,
        state: Option<Value>,
        message: Option<String>,
    ) -> Result<TaskResponse> {
        let url = self.build_url(&api_workflow_run(workflow_run_id)).await?;
        let response = self
            .client
            .patch(url.clone())
            .json(&json!({
                "status": status,
                "active_node_id": active_node_id,
                "state": state,
                "message": message
            }))
            .send()
            .await?;
        let response = Self::handle_response(url, response).await?;
        Ok(response.json::<TaskResponse>().await?)
    }

    pub async fn rename_workflow_run(
        &self,
        workflow_run_id: i64,
        name: Option<String>,
    ) -> Result<TaskResponse> {
        let url = self
            .build_url(&api_workflow_run_rename(workflow_run_id))
            .await?;
        let response = self
            .client
            .post(url.clone())
            .json(&json!({ "name": name }))
            .send()
            .await?;
        let response = Self::handle_response(url, response).await?;
        Ok(response.json::<TaskResponse>().await?)
    }

    pub async fn pause_workflow_run(&self, workflow_run_id: i64) -> Result<TaskResponse> {
        self.post_workflow_run_command(workflow_run_id, "pause")
            .await
    }

    pub async fn resume_workflow_run(&self, workflow_run_id: i64) -> Result<TaskResponse> {
        self.post_workflow_run_command(workflow_run_id, "resume")
            .await
    }

    pub async fn cancel_workflow_run(&self, workflow_run_id: i64) -> Result<TaskResponse> {
        self.post_workflow_run_command(workflow_run_id, "cancel")
            .await
    }

    pub async fn replay_workflow_run(
        &self,
        workflow_run_id: i64,
        from_step_id: Option<String>,
    ) -> Result<WorkflowRun> {
        let url = self
            .build_url(&api_workflow_run_replay(workflow_run_id))
            .await?;
        let response = self
            .client
            .post(url.clone())
            .json(&json!({ "from_step_id": from_step_id }))
            .send()
            .await?;
        let response = Self::handle_response(url, response).await?;
        let body = response.json::<Value>().await?;
        serde_json::from_value(
            body.get("run")
                .cloned()
                .ok_or_else(|| ApiError::UnexpectedResponse("missing run".into()))?
                .into(),
        )
        .map_err(|err| ApiError::UnexpectedResponse(err.to_string()))
    }

    async fn post_workflow_run_command(
        &self,
        workflow_run_id: i64,
        command: &str,
    ) -> Result<TaskResponse> {
        let url = self
            .build_url(&api_workflow_run_command(workflow_run_id, command))
            .await?;
        let response = self
            .client
            .post(url.clone())
            .json(&json!({}))
            .send()
            .await?;
        let response = Self::handle_response(url, response).await?;
        Ok(response.json::<TaskResponse>().await?)
    }

    pub async fn enqueue_action_dispatch(
        &self,
        dedupe_key: &str,
        command: &ActionCommand,
    ) -> Result<ActionDispatchRecord> {
        let url = self.build_url(API_SCHEDULER_ACTION_DISPATCHES).await?;
        let response = self
            .client
            .post(url.clone())
            .json(&json!({
                "dedupe_key": dedupe_key,
                "command": command,
            }))
            .send()
            .await?;
        let response = Self::handle_response(url, response).await?;
        Ok(response.json::<ActionDispatchRecord>().await?)
    }

    pub async fn fetch_pending_action_dispatches(
        &self,
        limit: i64,
    ) -> Result<Vec<ActionDispatchRecord>> {
        let mut url = self
            .build_url(API_SCHEDULER_ACTION_DISPATCHES_PENDING)
            .await?;
        url.query_pairs_mut()
            .append_pair("limit", &limit.to_string());
        let response = self.client.get(url.clone()).send().await?;
        let response = Self::handle_response(url, response).await?;
        Ok(response.json::<Vec<ActionDispatchRecord>>().await?)
    }

    pub async fn claim_ready_nodes(
        &self,
        scheduler_id: &str,
        lease_until: DateTime<Utc>,
        limit: i64,
    ) -> Result<Vec<ReadyNodeRecord>> {
        let url = self.build_url(API_SCHEDULER_READY_NODES_CLAIM).await?;
        let response = self
            .client
            .post(url.clone())
            .json(&json!({
                "scheduler_id": scheduler_id,
                "lease_until": lease_until,
                "limit": limit,
            }))
            .send()
            .await?;
        let response = Self::handle_response(url, response).await?;
        Ok(response.json::<Vec<ReadyNodeRecord>>().await?)
    }

    pub async fn process_ready_node(
        &self,
        ready_node_id: i64,
        scheduler_id: &str,
        workflow_run_id: Option<i64>,
        node_id: Option<String>,
        next_ready_at: Option<DateTime<Utc>>,
    ) -> Result<TaskResponse> {
        let url = self
            .build_url(&api_scheduler_ready_node_process(ready_node_id))
            .await?;
        let response = self
            .client
            .post(url.clone())
            .json(&json!({
                "scheduler_id": scheduler_id,
                "workflow_run_id": workflow_run_id,
                "node_id": node_id,
                "next_ready_at": next_ready_at,
            }))
            .send()
            .await?;
        let response = Self::handle_response(url, response).await?;
        Ok(response.json::<TaskResponse>().await?)
    }

    pub async fn claim_pending_action_dispatches(
        &self,
        scheduler_id: &str,
        lease_until: DateTime<Utc>,
        limit: i64,
    ) -> Result<Vec<ActionDispatchRecord>> {
        let url = self
            .build_url(API_SCHEDULER_ACTION_DISPATCHES_CLAIM)
            .await?;
        let response = self
            .client
            .post(url.clone())
            .json(&json!({
                "scheduler_id": scheduler_id,
                "lease_until": lease_until,
                "limit": limit,
            }))
            .send()
            .await?;
        let response = Self::handle_response(url, response).await?;
        Ok(response.json::<Vec<ActionDispatchRecord>>().await?)
    }

    pub async fn mark_action_dispatch_published(&self, dispatch_id: i64) -> Result<TaskResponse> {
        let url = self
            .build_url(&api_scheduler_action_dispatch_published(dispatch_id))
            .await?;
        let response = self.client.post(url.clone()).send().await?;
        let response = Self::handle_response(url, response).await?;
        Ok(response.json::<TaskResponse>().await?)
    }

    pub async fn mark_action_dispatch_failed(
        &self,
        dispatch_id: i64,
        error: &str,
    ) -> Result<TaskResponse> {
        let url = self
            .build_url(&api_scheduler_action_dispatch_failed(dispatch_id))
            .await?;
        let response = self
            .client
            .post(url.clone())
            .json(&json!({ "error": error }))
            .send()
            .await?;
        let response = Self::handle_response(url, response).await?;
        Ok(response.json::<TaskResponse>().await?)
    }

    pub async fn fetch_workflow_run(
        &self,
        workflow_run_id: i64,
    ) -> Result<(WorkflowRun, Vec<WorkflowNodeRun>)> {
        let url = self.build_url(&api_workflow_run(workflow_run_id)).await?;
        let response = self.client.get(url.clone()).send().await?;
        let response = Self::handle_response(url, response).await?;
        let body = response.json::<Value>().await?;
        let run = serde_json::from_value(
            body.get("run")
                .cloned()
                .ok_or_else(|| ApiError::UnexpectedResponse("missing run".into()))?
                .into(),
        )
        .map_err(|err| ApiError::UnexpectedResponse(err.to_string()))?;
        let nodes = serde_json::from_value(
            body.get("nodes")
                .cloned()
                .unwrap_or(Value::Array(vec![]))
                .into(),
        )
        .map_err(|err| ApiError::UnexpectedResponse(err.to_string()))?;
        Ok((run, nodes))
    }

    pub async fn create_workflow_node_run(
        &self,
        workflow_run_id: i64,
        node_id: &str,
        parameters: Value,
    ) -> Result<WorkflowNodeRun> {
        let url = self
            .build_url(&api_workflow_run_nodes(workflow_run_id))
            .await?;
        let response = self
            .client
            .post(url.clone())
            .json(&json!({ "node_id": node_id, "parameters": parameters }))
            .send()
            .await?;
        let response = Self::handle_response(url, response).await?;
        Ok(response.json::<WorkflowNodeRun>().await?)
    }

    #[allow(clippy::too_many_arguments)]
    pub async fn update_workflow_node_run(
        &self,
        node_run_id: i64,
        status: WorkflowStatus,
        attempt: Option<i64>,
        parameters: Option<Value>,
        output_json: Option<Value>,
        state: Option<Value>,
        transition_reason: Option<String>,
        message: Option<String>,
    ) -> Result<TaskResponse> {
        let url = self.build_url(&api_workflow_node_run(node_run_id)).await?;
        let response = self
            .client
            .patch(url.clone())
            .json(&json!({
                "status": status,
                "attempt": attempt,
                "parameters": parameters,
                "output_json": output_json,
                "state": state,
                "transition_reason": transition_reason,
                "message": message
            }))
            .send()
            .await?;
        let response = Self::handle_response(url, response).await?;
        Ok(response.json::<TaskResponse>().await?)
    }

    pub async fn set_workflow_node_run_status(
        &self,
        node_run_id: i64,
        payload: &WorkflowNodeRunStatusPayload,
    ) -> Result<TaskResponse> {
        let url = self.build_url(&api_workflow_node_run(node_run_id)).await?;
        let response = self.client.patch(url.clone()).json(payload).send().await?;
        let response = Self::handle_response(url, response).await?;
        Ok(response.json::<TaskResponse>().await?)
    }

    pub async fn append_workflow_node_run_chunk(
        &self,
        node_run_id: i64,
        payload: &RunChunkPayload,
    ) -> Result<Vec<WorkflowNodeRunChunk>> {
        let url = self
            .build_url(&api_workflow_node_run_chunks(node_run_id))
            .await?;
        let response = self.client.post(url.clone()).json(payload).send().await?;
        let response = Self::handle_response(url, response).await?;
        Ok(response.json::<Vec<WorkflowNodeRunChunk>>().await?)
    }

    pub async fn fetch_workflow_node_run_chunks(
        &self,
        node_run_id: i64,
        cursor: Option<i64>,
        limit: i64,
    ) -> Result<Vec<WorkflowNodeRunChunk>> {
        let mut url = self
            .build_url(&api_workflow_node_run_chunks(node_run_id))
            .await?;
        url.query_pairs_mut()
            .append_pair("limit", &limit.to_string());
        if let Some(cursor) = cursor {
            url.query_pairs_mut()
                .append_pair("cursor", &cursor.to_string());
        }
        let response = self.client.get(url.clone()).send().await?;
        let response = Self::handle_response(url, response).await?;
        Ok(response.json::<Vec<WorkflowNodeRunChunk>>().await?)
    }

    pub async fn add_workflow_node_run_artifact(
        &self,
        node_run_id: i64,
        payload: &RunArtifactPayload,
    ) -> Result<Vec<WorkflowNodeRunArtifact>> {
        let url = self
            .build_url(&api_workflow_node_run_artifacts(node_run_id))
            .await?;
        let response = self.client.post(url.clone()).json(payload).send().await?;
        let response = Self::handle_response(url, response).await?;
        Ok(response.json::<Vec<WorkflowNodeRunArtifact>>().await?)
    }

    pub async fn fetch_supervisor_status(&self) -> Result<Value> {
        let url = self.build_url(API_SUPERVISOR_STATUS).await?;
        let response = self.client.get(url.clone()).send().await?;
        let response = Self::handle_response(url, response).await?;
        Ok(response.json::<Value>().await?)
    }

    pub async fn fetch_approvals(&self, workflow_run_id: Option<i64>) -> Result<Vec<Value>> {
        let mut url = self.build_url(API_APPROVALS).await?;
        if let Some(workflow_run_id) = workflow_run_id {
            url.query_pairs_mut()
                .append_pair("workflow_run_id", &workflow_run_id.to_string());
        }
        let response = self.client.get(url.clone()).send().await?;
        let response = Self::handle_response(url, response).await?;
        Ok(response.json::<Vec<Value>>().await?)
    }

    pub async fn approve_request(
        &self,
        approval_id: i64,
        resolved_by: Option<String>,
        message: Option<String>,
        output_json: Option<Value>,
    ) -> Result<Value> {
        self.resolve_approval(approval_id, true, resolved_by, message, output_json)
            .await
    }

    pub async fn reject_request(
        &self,
        approval_id: i64,
        resolved_by: Option<String>,
        message: Option<String>,
        output_json: Option<Value>,
    ) -> Result<Value> {
        self.resolve_approval(approval_id, false, resolved_by, message, output_json)
            .await
    }

    async fn resolve_approval(
        &self,
        approval_id: i64,
        approved: bool,
        resolved_by: Option<String>,
        message: Option<String>,
        output_json: Option<Value>,
    ) -> Result<Value> {
        let command = if approved { "approve" } else { "reject" };
        let url = self
            .build_url(&api_approval_command(approval_id, command))
            .await?;
        let response = self
            .client
            .post(url.clone())
            .json(&json!({
                "resolved_by": resolved_by,
                "message": message,
                "output_json": output_json
            }))
            .send()
            .await?;
        let response = Self::handle_response(url, response).await?;
        Ok(response.json::<Value>().await?)
    }

    pub async fn create_automation_record(&self, path: &str, record: Value) -> Result<Value> {
        let url = self.build_url(path).await?;
        let response = self.client.post(url.clone()).json(&record).send().await?;
        let response = Self::handle_response(url, response).await?;
        Ok(response.json::<Value>().await?)
    }

    pub async fn fetch_idempotency_key(&self, scope: &str, key: &str) -> Result<Option<Value>> {
        let url = self
            .build_url(&format!("{API_IDEMPOTENCY_KEYS}?scope={scope}&key={key}"))
            .await?;
        let response = self.client.get(url.clone()).send().await?;
        if response.status() == reqwest::StatusCode::NOT_FOUND {
            return Ok(None);
        }
        let response = Self::handle_response(url, response).await?;
        Ok(Some(response.json::<Value>().await?))
    }

    pub async fn put_idempotency_key(
        &self,
        scope: &str,
        key: &str,
        result: Value,
    ) -> Result<Value> {
        let url = self.build_url(API_IDEMPOTENCY_KEYS).await?;
        let response = self
            .client
            .post(url.clone())
            .json(&json!({ "scope": scope, "key": key, "result": result }))
            .send()
            .await?;
        let response = Self::handle_response(url, response).await?;
        Ok(response.json::<Value>().await?)
    }

    pub async fn fetch_credential(&self, scope: &str, name: &str) -> Result<String> {
        let mut url = self.build_url(API_CREDENTIALS).await?;
        url.query_pairs_mut()
            .append_pair("scope", scope)
            .append_pair("name", name);
        let response = self.client.get(url.clone()).send().await?;
        let response = Self::handle_response(url, response).await?;
        let body = response.json::<Value>().await?;
        body.get("secret")
            .and_then(Value::as_str)
            .map(str::to_owned)
            .ok_or_else(|| ApiError::UnexpectedResponse("missing credential secret".into()))
    }

    /// list every stored setting (secrets and config) without their values.
    pub async fn list_settings(&self) -> Result<Vec<SettingSummary>> {
        let url = self.build_url(API_CREDENTIALS).await?;
        let response = self.client.get(url.clone()).send().await?;
        let response = Self::handle_response(url, response).await?;
        Ok(response.json::<Vec<SettingSummary>>().await?)
    }

    /// fetch a single setting's value. config returns parsed json; secrets return a string.
    pub async fn get_setting(&self, kind: SettingKind, scope: &str, name: &str) -> Result<Value> {
        let mut url = self.build_url(API_CREDENTIALS).await?;
        url.query_pairs_mut()
            .append_pair("kind", kind.as_str())
            .append_pair("scope", scope)
            .append_pair("name", name);
        let response = self.client.get(url.clone()).send().await?;
        let response = Self::handle_response(url, response).await?;
        let body = response.json::<Value>().await?;
        body.get("value")
            .cloned()
            .ok_or_else(|| ApiError::UnexpectedResponse("missing setting value".into()))
    }

    /// store a setting value of the given kind. config values carry a declared json-schema
    /// (required once per slot) validated by the web service.
    pub async fn put_setting(
        &self,
        kind: SettingKind,
        scope: &str,
        name: &str,
        value: &Value,
        schema: Option<&Value>,
    ) -> Result<Value> {
        let url = self.build_url(API_CREDENTIALS).await?;
        let mut body = json!({
            "scope": scope,
            "name": name,
            "value": value,
            "kind": kind.as_str(),
        });
        if let (Some(schema), Some(object)) = (schema, body.as_object_mut()) {
            object.insert("schema".into(), schema.clone());
        }
        let response = self.client.post(url.clone()).json(&body).send().await?;
        let response = Self::handle_response(url, response).await?;
        Ok(response.json::<Value>().await?)
    }

    /// delete a setting of the given kind.
    pub async fn delete_setting(
        &self,
        kind: SettingKind,
        scope: &str,
        name: &str,
    ) -> Result<Value> {
        let mut url = self.build_url(API_CREDENTIALS).await?;
        url.query_pairs_mut()
            .append_pair("kind", kind.as_str())
            .append_pair("scope", scope)
            .append_pair("name", name);
        let response = self.client.delete(url.clone()).send().await?;
        let response = Self::handle_response(url, response).await?;
        Ok(response.json::<Value>().await?)
    }

    /// Record execution metadata for a scheduled task run.
    pub async fn log_task_run(&self) -> Result<TaskResponse> {
        Err(ApiError::UnexpectedResponse("deprecated".into()))
    }

    async fn build_url(&self, path: &str) -> Result<Url> {
        let base = self
            .locator
            .wait_for_service_url()
            .await
            .map_err(ApiError::discovery)?;
        let base_url = Url::parse(&base).map_err(|source| ApiError::InvalidBaseUrl {
            url: base.clone(),
            source,
        })?;
        let trimmed_path = path.trim_start_matches('/');
        base_url
            .join(trimmed_path)
            .map_err(|source| ApiError::InvalidPath {
                base: base_url.clone(),
                path: trimmed_path.to_string(),
                source,
            })
    }

    async fn handle_response(url: Url, response: Response) -> Result<Response> {
        let status = response.status();
        if status.is_success() {
            Ok(response)
        } else {
            let message = response
                .text()
                .await
                .unwrap_or_else(|_| "<unable to read body>".into());
            Err(ApiError::Http {
                status,
                url,
                message,
            })
        }
    }
}
