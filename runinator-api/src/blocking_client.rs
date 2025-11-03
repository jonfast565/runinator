use reqwest::{
    blocking::{Client, Response},
    Url,
};
use runinator_models::{core::ScheduledTask, web::TaskResponse};

use crate::{
    error::{ApiError, Result},
    locator::BlockingServiceLocator,
    types::TaskRunPayload,
};

/// Blocking API client that wraps `reqwest::blocking::Client`.
#[derive(Clone)]
pub struct BlockingApiClient<L> {
    client: Client,
    locator: L,
}

impl<L> BlockingApiClient<L>
where
    L: BlockingServiceLocator,
{
    /// Construct a client with the default `reqwest::blocking::Client` configuration.
    pub fn new(locator: L) -> reqwest::Result<Self> {
        let client = Client::builder().build()?;
        Ok(Self { client, locator })
    }

    /// Construct a client using a preconfigured HTTP client instance.
    pub fn with_client(locator: L, client: Client) -> Self {
        Self { client, locator }
    }

    /// Fetch all scheduled tasks from the web service.
    pub fn fetch_tasks(&self) -> Result<Vec<ScheduledTask>> {
        let url = self.build_url("/tasks")?;
        let response = self.client.get(url.clone()).send()?;
        let response = Self::handle_response(url, response)?;
        Ok(response.json::<Vec<ScheduledTask>>()?)
    }

    /// Create or replace a scheduled task.
    pub fn upsert_task(&self, task: &ScheduledTask) -> Result<TaskResponse> {
        let url = self.build_url("/tasks")?;
        let response = self.client.post(url.clone()).json(task).send()?;
        let response = Self::handle_response(url, response)?;
        Ok(response.json::<TaskResponse>()?)
    }

    /// Update an existing scheduled task by identifier.
    pub fn update_task(&self, task: &ScheduledTask) -> Result<TaskResponse> {
        let id = task.id.ok_or(ApiError::MissingTaskId)?;
        let url = self.build_url(&format!("/tasks/{id}"))?;
        let response = self.client.patch(url.clone()).json(task).send()?;
        let response = Self::handle_response(url, response)?;
        Ok(response.json::<TaskResponse>()?)
    }

    /// Delete a scheduled task and return the service acknowledgement.
    pub fn delete_task(&self, task_id: i64) -> Result<TaskResponse> {
        let url = self.build_url(&format!("/tasks/{task_id}"))?;
        let response = self.client.delete(url.clone()).send()?;
        let response = Self::handle_response(url, response)?;
        Ok(response.json::<TaskResponse>()?)
    }

    /// Request an immediate run for a scheduled task.
    pub fn request_task_run(&self, task_id: i64) -> Result<TaskResponse> {
        let url = self.build_url(&format!("/tasks/{task_id}/request_run"))?;
        let response = self.client.post(url.clone()).send()?;
        let response = Self::handle_response(url, response)?;
        Ok(response.json::<TaskResponse>()?)
    }

    /// Record execution metadata for a scheduled task run.
    pub fn log_task_run(&self, payload: &TaskRunPayload) -> Result<TaskResponse> {
        let url = self.build_url("/task_runs")?;
        let response = self.client.post(url.clone()).json(payload).send()?;
        let response = Self::handle_response(url, response)?;
        Ok(response.json::<TaskResponse>()?)
    }

    fn build_url(&self, path: &str) -> Result<Url> {
        let base = self
            .locator
            .wait_for_service_url()
            .map_err(ApiError::discovery)?;
        let base_url = Url::parse(&base).map_err(|source| ApiError::InvalidBaseUrl {
            url: base.clone(),
            source,
        })?;
        let trimmed_path = path.trim_start_matches('/');
        base_url.join(trimmed_path).map_err(|source| ApiError::InvalidPath {
            base: base_url.clone(),
            path: trimmed_path.to_string(),
            source,
        })
    }

    fn handle_response(url: Url, response: Response) -> Result<Response> {
        let status = response.status();
        if status.is_success() {
            Ok(response)
        } else {
            let message = response
                .text()
                .unwrap_or_else(|_| "<unable to read body>".into());
            Err(ApiError::Http { status, url, message })
        }
    }
}
