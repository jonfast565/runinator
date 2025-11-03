use std::{sync::Arc, time::Duration};

use chrono::{DateTime, Utc};
use log::debug;
use reqwest::Url;
use runinator_models::{
    core::ScheduledTask,
    errors::{RuntimeError, SendableError},
};
use serde::Serialize;

use crate::worker_comm::WorkerManager;

#[derive(Clone)]
pub struct SchedulerApi {
    client: reqwest::Client,
    worker_manager: Arc<WorkerManager>,
}

#[derive(Serialize)]
struct TaskRunRequest {
    task_id: i64,
    started_at: DateTime<Utc>,
    duration_ms: i64,
    #[serde(skip_serializing_if = "Option::is_none")]
    message: Option<String>,
}

impl SchedulerApi {
    pub fn new(
        worker_manager: Arc<WorkerManager>,
        timeout: Duration,
    ) -> Result<Self, SendableError> {
        let client = reqwest::Client::builder()
            .timeout(timeout)
            .build()
            .map_err(|err| -> SendableError { Box::new(err) })?;

        Ok(Self {
            client,
            worker_manager,
        })
    }

    pub async fn fetch_tasks(&self) -> Result<Vec<ScheduledTask>, SendableError> {
        let url = self.build_url("/tasks").await?;
        let response = self
            .client
            .get(url.clone())
            .send()
            .await
            .map_err(|err| -> SendableError { Box::new(err) })?;
        let response = handle_response(url, response).await?;
        let tasks = response
            .json::<Vec<ScheduledTask>>()
            .await
            .map_err(|err| -> SendableError { Box::new(err) })?;
        debug!("Fetched {} task(s) from API", tasks.len());
        Ok(tasks)
    }

    pub async fn update_task(&self, task: &ScheduledTask) -> Result<(), SendableError> {
        let id = task.id.ok_or_else(|| {
            RuntimeError::new(
                "scheduler.api.update.missing_id".into(),
                "Task must contain an ID before update".into(),
            )
        })?;
        let url = self.build_url(&format!("/tasks/{id}")).await?;
        let response = self
            .client
            .patch(url.clone())
            .json(task)
            .send()
            .await
            .map_err(|err| -> SendableError { Box::new(err) })?;
        handle_response(url, response).await?;
        Ok(())
    }

    pub async fn log_task_run(
        &self,
        task_id: i64,
        started_at: DateTime<Utc>,
        duration_ms: i64,
        message: Option<String>,
    ) -> Result<(), SendableError> {
        let url = self.build_url("/task_runs").await?;
        let payload = TaskRunRequest {
            task_id,
            started_at,
            duration_ms,
            message,
        };

        let response = self
            .client
            .post(url.clone())
            .json(&payload)
            .send()
            .await
            .map_err(|err| -> SendableError { Box::new(err) })?;
        handle_response(url, response).await?;
        Ok(())
    }

    async fn build_url(&self, path: &str) -> Result<Url, SendableError> {
        let base = self.worker_manager.wait_for_service_url().await?;
        let base_url = Url::parse(&base).map_err(|err| -> SendableError { Box::new(err) })?;
        let joined = base_url
            .join(path.trim_start_matches('/'))
            .map_err(|err| -> SendableError { Box::new(err) })?;
        Ok(joined)
    }
}

async fn handle_response(
    url: Url,
    response: reqwest::Response,
) -> Result<reqwest::Response, SendableError> {
    let status = response.status();
    if status.is_success() {
        Ok(response)
    } else {
        let body = response
            .text()
            .await
            .unwrap_or_else(|_| "<unable to read body>".into());
        Err(Box::new(RuntimeError::new(
            format!("scheduler.api.{}", status.as_u16()),
            format!("{} {}: {}", status.as_str(), url, body),
        )))
    }
}
