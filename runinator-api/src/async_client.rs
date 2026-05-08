use reqwest::{Client, Response, Url};
use runinator_models::{
    core::ScheduledTask,
    runs::{RunRequest, RunStatus, RunSummary},
    web::TaskResponse,
    workflows::{WorkflowDefinition, WorkflowRun, WorkflowStepRun},
};
use serde_json::{json, Value};

use crate::{
    error::{ApiError, Result},
    locator::ServiceLocator,
    types::{RunArtifactPayload, RunChunkPayload, RunStatusPayload, TaskRunPayload},
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

    /// Fetch all scheduled tasks from the web service.
    pub async fn fetch_tasks(&self) -> Result<Vec<ScheduledTask>> {
        let url = self.build_url("/tasks").await?;
        let response = self.client.get(url.clone()).send().await?;
        let response = Self::handle_response(url, response).await?;
        Ok(response.json::<Vec<ScheduledTask>>().await?)
    }

    /// Create or replace a scheduled task.
    pub async fn upsert_task(&self, task: &ScheduledTask) -> Result<TaskResponse> {
        let url = self.build_url("/tasks").await?;
        let response = self.client.post(url.clone()).json(task).send().await?;
        let response = Self::handle_response(url, response).await?;
        Ok(response.json::<TaskResponse>().await?)
    }

    /// Update an existing scheduled task by identifier.
    pub async fn update_task(&self, task: &ScheduledTask) -> Result<TaskResponse> {
        let id = task.id.ok_or(ApiError::MissingTaskId)?;
        let url = self.build_url(&format!("/tasks/{id}")).await?;
        let response = self.client.patch(url.clone()).json(task).send().await?;
        let response = Self::handle_response(url, response).await?;
        Ok(response.json::<TaskResponse>().await?)
    }

    /// Delete a scheduled task and return the service acknowledgement.
    pub async fn delete_task(&self, task_id: i64) -> Result<TaskResponse> {
        let url = self.build_url(&format!("/tasks/{task_id}")).await?;
        let response = self.client.delete(url.clone()).send().await?;
        let response = Self::handle_response(url, response).await?;
        Ok(response.json::<TaskResponse>().await?)
    }

    /// Request an immediate run for a scheduled task.
    pub async fn request_task_run(&self, task_id: i64) -> Result<TaskResponse> {
        let url = self
            .build_url(&format!("/tasks/{task_id}/request_run"))
            .await?;
        let response = self.client.post(url.clone()).send().await?;
        let response = Self::handle_response(url, response).await?;
        Ok(response.json::<TaskResponse>().await?)
    }

    pub async fn create_run(&self, task_id: i64, request: &RunRequest) -> Result<RunSummary> {
        let url = self.build_url(&format!("/tasks/{task_id}/runs")).await?;
        let response = self.client.post(url.clone()).json(request).send().await?;
        let response = Self::handle_response(url, response).await?;
        Ok(response.json::<RunSummary>().await?)
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

    pub async fn fetch_workflow_runs_by_status(
        &self,
        status: RunStatus,
    ) -> Result<Vec<WorkflowRun>> {
        let url = self
            .build_url(&format!("/workflow_runs?status={}", status.as_str()))
            .await?;
        let response = self.client.get(url.clone()).send().await?;
        let response = Self::handle_response(url, response).await?;
        Ok(response.json::<Vec<WorkflowRun>>().await?)
    }

    pub async fn update_workflow_run(
        &self,
        workflow_run_id: i64,
        status: RunStatus,
        message: Option<String>,
    ) -> Result<TaskResponse> {
        let url = self
            .build_url(&format!("/workflow_runs/{workflow_run_id}"))
            .await?;
        let response = self
            .client
            .patch(url.clone())
            .json(&json!({ "status": status, "message": message }))
            .send()
            .await?;
        let response = Self::handle_response(url, response).await?;
        Ok(response.json::<TaskResponse>().await?)
    }

    pub async fn fetch_workflow_run(
        &self,
        workflow_run_id: i64,
    ) -> Result<(WorkflowRun, Vec<WorkflowStepRun>)> {
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
        let steps =
            serde_json::from_value(body.get("steps").cloned().unwrap_or(Value::Array(vec![])))
                .map_err(|err| ApiError::UnexpectedResponse(err.to_string()))?;
        Ok((run, steps))
    }

    pub async fn create_workflow_step_run(
        &self,
        workflow_run_id: i64,
        step_id: &str,
        parameters: Value,
    ) -> Result<WorkflowStepRun> {
        let url = self
            .build_url(&format!("/workflow_runs/{workflow_run_id}/steps"))
            .await?;
        let response = self
            .client
            .post(url.clone())
            .json(&json!({ "step_id": step_id, "parameters": parameters }))
            .send()
            .await?;
        let response = Self::handle_response(url, response).await?;
        Ok(response.json::<WorkflowStepRun>().await?)
    }

    pub async fn update_workflow_step_run(
        &self,
        step_run_id: i64,
        status: RunStatus,
        task_run_id: Option<i64>,
        attempt: Option<i64>,
        parameters: Option<Value>,
        message: Option<String>,
    ) -> Result<TaskResponse> {
        let url = self
            .build_url(&format!("/workflow_step_runs/{step_run_id}"))
            .await?;
        let response = self
            .client
            .patch(url.clone())
            .json(&json!({
                "status": status,
                "task_run_id": task_run_id,
                "attempt": attempt,
                "parameters": parameters,
                "message": message
            }))
            .send()
            .await?;
        let response = Self::handle_response(url, response).await?;
        Ok(response.json::<TaskResponse>().await?)
    }

    /// Record execution metadata for a scheduled task run.
    pub async fn log_task_run(&self, payload: &TaskRunPayload) -> Result<TaskResponse> {
        let url = self.build_url("/task_runs").await?;
        let response = self.client.post(url.clone()).json(payload).send().await?;
        let response = Self::handle_response(url, response).await?;
        Ok(response.json::<TaskResponse>().await?)
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
