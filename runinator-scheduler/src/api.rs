use std::{sync::Arc, time::Duration};

use chrono::{DateTime, Utc};
use log::debug;
use runinator_api::{AsyncApiClient, TaskRunPayload};
use runinator_models::{
    core::ScheduledTask,
    errors::{RuntimeError, SendableError},
};

use crate::worker_comm::WorkerManager;

#[derive(Clone)]
pub struct SchedulerApi {
    client: AsyncApiClient<WorkerManager>,
}

impl SchedulerApi {
    pub fn new(
        worker_manager: Arc<WorkerManager>,
        timeout: Duration,
    ) -> Result<Self, SendableError> {
        let http_client = reqwest::Client::builder()
            .timeout(timeout)
            .build()
            .map_err(|err| -> SendableError { Box::new(err) })?;

        Ok(Self {
            client: AsyncApiClient::with_client(worker_manager.as_ref().clone(), http_client),
        })
    }

    pub async fn fetch_tasks(&self) -> Result<Vec<ScheduledTask>, SendableError> {
        let tasks = self
            .client
            .fetch_tasks()
            .await
            .map_err(|err| -> SendableError { Box::new(err) })?;
        debug!("Fetched {} task(s) from API", tasks.len());
        Ok(tasks)
    }

    pub async fn update_task(&self, task: &ScheduledTask) -> Result<(), SendableError> {
        if task.id.is_none() {
            return Err(Box::new(RuntimeError::new(
                "scheduler.api.update.missing_id".into(),
                "Task must contain an ID before update".into(),
            )));
        }
        let _ = self
            .client
            .update_task(task)
            .await
            .map_err(|err| -> SendableError { Box::new(err) })?;
        Ok(())
    }

    pub async fn log_task_run(
        &self,
        task_id: i64,
        started_at: DateTime<Utc>,
        duration_ms: i64,
        message: Option<String>,
    ) -> Result<(), SendableError> {
        let payload = TaskRunPayload {
            task_id,
            started_at,
            duration_ms,
            message,
        };

        let _ = self
            .client
            .log_task_run(&payload)
            .await
            .map_err(|err| -> SendableError { Box::new(err) })?;
        Ok(())
    }
}
