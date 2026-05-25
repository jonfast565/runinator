use chrono::{DateTime, Utc};
use reqwest::{Client, Response, Url};
use runinator_comm::{ActionCommand, ActionDispatchRecord};
use runinator_models::{
    bundles::{Bundle, ProviderBundle, SecretBundle},
    providers::ProviderMetadata,
    runs::{RunStatus, RunSummary},
    web::TaskResponse,
    workflows::{
        WorkflowBundle, WorkflowDefinition, WorkflowNodeRun, WorkflowNodeRunArtifact,
        WorkflowNodeRunChunk, WorkflowRun, WorkflowStatus, WorkflowTrigger,
    },
};
use serde_json::{json, Value};

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
        let url = self.build_url("/providers").await?;
        let response = self.client.get(url.clone()).send().await?;
        let response = Self::handle_response(url, response).await?;
        Ok(response.json::<Vec<ProviderMetadata>>().await?)
    }

    /// Register provider/action metadata with the web service.
    pub async fn upsert_provider(&self, provider: &ProviderMetadata) -> Result<ProviderMetadata> {
        let url = self.build_url("/providers").await?;
        let response = self.client.post(url.clone()).json(provider).send().await?;
        let response = Self::handle_response(url, response).await?;
        Ok(response.json::<ProviderMetadata>().await?)
    }

    pub async fn fetch_run(&self, run_id: i64) -> Result<RunSummary> {
        let url = self.build_url(&format!("/runs/{run_id}")).await?;
        let response = self.client.get(url.clone()).send().await?;
        let response = Self::handle_response(url, response).await?;
        Ok(response.json::<RunSummary>().await?)
    }

    pub async fn fetch_runs_by_status(&self, status: RunStatus) -> Result<Vec<RunSummary>> {
        let url = self
            .build_url(&format!("/runs?status={}", status.as_str()))
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
        let url = self.build_url(&format!("/runs/{run_id}")).await?;
        let response = self.client.patch(url.clone()).json(payload).send().await?;
        let response = Self::handle_response(url, response).await?;
        Ok(response.json::<TaskResponse>().await?)
    }

    pub async fn append_run_chunk(
        &self,
        run_id: i64,
        payload: &RunChunkPayload,
    ) -> Result<TaskResponse> {
        let url = self.build_url(&format!("/runs/{run_id}/chunks")).await?;
        let response = self.client.post(url.clone()).json(payload).send().await?;
        let response = Self::handle_response(url, response).await?;
        Ok(response.json::<TaskResponse>().await?)
    }

    pub async fn add_run_artifact(
        &self,
        run_id: i64,
        payload: &RunArtifactPayload,
    ) -> Result<TaskResponse> {
        let url = self.build_url(&format!("/runs/{run_id}/artifacts")).await?;
        let response = self.client.post(url.clone()).json(payload).send().await?;
        let response = Self::handle_response(url, response).await?;
        Ok(response.json::<TaskResponse>().await?)
    }

    pub async fn fetch_workflow(&self, workflow_id: i64) -> Result<WorkflowDefinition> {
        let url = self.build_url(&format!("/workflows/{workflow_id}")).await?;
        let response = self.client.get(url.clone()).send().await?;
        let response = Self::handle_response(url, response).await?;
        Ok(response.json::<WorkflowDefinition>().await?)
    }

    pub async fn fetch_workflows(&self) -> Result<Vec<WorkflowDefinition>> {
        let url = self.build_url("/workflows").await?;
        let response = self.client.get(url.clone()).send().await?;
        let response = Self::handle_response(url, response).await?;
        Ok(response.json::<Vec<WorkflowDefinition>>().await?)
    }

    pub async fn fetch_workflow_by_name(&self, name: &str) -> Result<WorkflowDefinition> {
        let mut url = self.build_url("/workflows").await?;
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
            Some(id) => self.build_url(&format!("/workflows/{id}")).await?,
            None => self.build_url("/workflows").await?,
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
        let url = self.build_url("/workflows/validate").await?;
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

    pub async fn import_workflow_bundle(&self, bundle: &WorkflowBundle) -> Result<WorkflowBundle> {
        self.import_bundle(bundle).await
    }

    pub async fn import_provider_bundle(&self, bundle: &ProviderBundle) -> Result<ProviderBundle> {
        self.import_bundle(bundle).await
    }

    pub async fn import_secret_bundle(&self, bundle: &SecretBundle) -> Result<SecretBundle> {
        self.import_bundle(bundle).await
    }

    pub async fn export_workflow_bundle(&self, workflow_id: Option<i64>) -> Result<WorkflowBundle> {
        let path = workflow_id
            .map(|id| format!("/workflows/{id}/export"))
            .unwrap_or_else(|| "/workflows/export".into());
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
        let url = self
            .build_url(&format!("/workflows/{workflow_id}/triggers"))
            .await?;
        let response = self.client.get(url.clone()).send().await?;
        let response = Self::handle_response(url, response).await?;
        Ok(response.json::<Vec<WorkflowTrigger>>().await?)
    }

    pub async fn fetch_due_workflow_triggers(&self) -> Result<Vec<WorkflowTrigger>> {
        let url = self.build_url("/workflow_triggers/due").await?;
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
            .build_url("/scheduler/workflow_trigger_firings/claim")
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
        let url = self
            .build_url(&format!("/workflow_triggers/{trigger_id}"))
            .await?;
        let response = self.client.get(url.clone()).send().await?;
        let response = Self::handle_response(url, response).await?;
        Ok(response.json::<WorkflowTrigger>().await?)
    }

    pub async fn upsert_workflow_trigger(
        &self,
        trigger: &WorkflowTrigger,
    ) -> Result<WorkflowTrigger> {
        let url = match trigger.id {
            Some(id) => self.build_url(&format!("/workflow_triggers/{id}")).await?,
            None => {
                self.build_url(&format!("/workflows/{}/triggers", trigger.workflow_id))
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
        let url = self
            .build_url(&format!("/workflow_triggers/{trigger_id}"))
            .await?;
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
            .build_url(&format!("/workflow_triggers/{trigger_id}/runs"))
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
                .ok_or_else(|| ApiError::UnexpectedResponse("missing run".into()))?,
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
        let url = self
            .build_url(&format!("/workflows/{workflow_id}/runs"))
            .await?;
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
                .ok_or_else(|| ApiError::UnexpectedResponse("missing run".into()))?,
        )
        .map_err(|err| ApiError::UnexpectedResponse(err.to_string()))
    }

    pub async fn fetch_workflow_runs_by_status(
        &self,
        status: WorkflowStatus,
    ) -> Result<Vec<WorkflowRun>> {
        let url = self
            .build_url(&format!("/workflow_runs?status={}", status.as_str()))
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
        let url = self.build_url("/scheduler/workflow_runs/claim").await?;
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
            .build_url(&format!(
                "/scheduler/workflow_runs/{workflow_run_id}/claim/renew"
            ))
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
            .build_url(&format!(
                "/scheduler/workflow_runs/{workflow_run_id}/claim/release"
            ))
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
        let mut url = self.build_url("/workflow_runs").await?;
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
        let mut url = self.build_url("/workflow_runs").await?;
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
        let url = self
            .build_url(&format!("/workflow_runs/{workflow_run_id}"))
            .await?;
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
            .build_url(&format!("/workflow_runs/{workflow_run_id}/rename"))
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
            .build_url(&format!("/workflow_runs/{workflow_run_id}/replay"))
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
                .ok_or_else(|| ApiError::UnexpectedResponse("missing run".into()))?,
        )
        .map_err(|err| ApiError::UnexpectedResponse(err.to_string()))
    }

    async fn post_workflow_run_command(
        &self,
        workflow_run_id: i64,
        command: &str,
    ) -> Result<TaskResponse> {
        let url = self
            .build_url(&format!("/workflow_runs/{workflow_run_id}/{command}"))
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
        let url = self.build_url("/scheduler/action_dispatches").await?;
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
            .build_url("/scheduler/action_dispatches/pending")
            .await?;
        url.query_pairs_mut()
            .append_pair("limit", &limit.to_string());
        let response = self.client.get(url.clone()).send().await?;
        let response = Self::handle_response(url, response).await?;
        Ok(response.json::<Vec<ActionDispatchRecord>>().await?)
    }

    pub async fn mark_action_dispatch_published(&self, dispatch_id: i64) -> Result<TaskResponse> {
        let url = self
            .build_url(&format!(
                "/scheduler/action_dispatches/{dispatch_id}/published"
            ))
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
            .build_url(&format!(
                "/scheduler/action_dispatches/{dispatch_id}/failed"
            ))
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
        let url = self
            .build_url(&format!("/workflow_runs/{workflow_run_id}"))
            .await?;
        let response = self.client.get(url.clone()).send().await?;
        let response = Self::handle_response(url, response).await?;
        let body = response.json::<Value>().await?;
        let run = serde_json::from_value(
            body.get("run")
                .cloned()
                .ok_or_else(|| ApiError::UnexpectedResponse("missing run".into()))?,
        )
        .map_err(|err| ApiError::UnexpectedResponse(err.to_string()))?;
        let nodes =
            serde_json::from_value(body.get("nodes").cloned().unwrap_or(Value::Array(vec![])))
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
            .build_url(&format!("/workflow_runs/{workflow_run_id}/nodes"))
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
        let url = self
            .build_url(&format!("/workflow_node_runs/{node_run_id}"))
            .await?;
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
        let url = self
            .build_url(&format!("/workflow_node_runs/{node_run_id}"))
            .await?;
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
            .build_url(&format!("/workflow_node_runs/{node_run_id}/chunks"))
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
            .build_url(&format!("/workflow_node_runs/{node_run_id}/chunks"))
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
            .build_url(&format!("/workflow_node_runs/{node_run_id}/artifacts"))
            .await?;
        let response = self.client.post(url.clone()).json(payload).send().await?;
        let response = Self::handle_response(url, response).await?;
        Ok(response.json::<Vec<WorkflowNodeRunArtifact>>().await?)
    }

    pub async fn fetch_supervisor_status(&self) -> Result<Value> {
        let url = self.build_url("/supervisor/status").await?;
        let response = self.client.get(url.clone()).send().await?;
        let response = Self::handle_response(url, response).await?;
        Ok(response.json::<Value>().await?)
    }

    pub async fn fetch_approvals(&self, workflow_run_id: Option<i64>) -> Result<Vec<Value>> {
        let mut url = self.build_url("/approvals").await?;
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
            .build_url(&format!("/approvals/{approval_id}/{command}"))
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
            .build_url(&format!("/idempotency_keys?scope={scope}&key={key}"))
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
        let url = self.build_url("/idempotency_keys").await?;
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
        let mut url = self.build_url("/credentials").await?;
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
