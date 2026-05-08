use std::{sync::Arc, time::Duration};

use chrono::{DateTime, Utc};
use log::debug;
use runinator_api::{AsyncApiClient, TaskRunPayload};
use runinator_models::{
    core::ScheduledTask,
    errors::{RuntimeError, SendableError},
    runs::{RunRequest, RunStatus, RunSummary},
    workflows::{WorkflowDefinition, WorkflowNodeRun, WorkflowRun, WorkflowStatus},
};
use serde_json::Value;

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

    pub async fn create_run(
        &self,
        task_id: i64,
        parameters: Value,
        trigger: impl Into<String>,
    ) -> Result<RunSummary, SendableError> {
        let request = RunRequest {
            parameters,
            trigger: trigger.into(),
        };
        self.client
            .create_run(task_id, &request)
            .await
            .map_err(|err| -> SendableError { Box::new(err) })
    }

    pub async fn fetch_run(&self, run_id: i64) -> Result<RunSummary, SendableError> {
        self.client
            .fetch_run(run_id)
            .await
            .map_err(|err| -> SendableError { Box::new(err) })
    }

    pub async fn fetch_runs_by_status(
        &self,
        status: RunStatus,
    ) -> Result<Vec<RunSummary>, SendableError> {
        self.client
            .fetch_runs_by_status(status)
            .await
            .map_err(|err| -> SendableError { Box::new(err) })
    }

    pub async fn fetch_workflow(
        &self,
        workflow_id: i64,
    ) -> Result<WorkflowDefinition, SendableError> {
        self.client
            .fetch_workflow(workflow_id)
            .await
            .map_err(|err| -> SendableError { Box::new(err) })
    }

    pub async fn create_workflow_run(
        &self,
        workflow_id: i64,
        parameters: Value,
    ) -> Result<WorkflowRun, SendableError> {
        self.client
            .create_workflow_run(workflow_id, parameters)
            .await
            .map_err(|err| -> SendableError { Box::new(err) })
    }

    pub async fn fetch_workflow_runs_by_status(
        &self,
        status: WorkflowStatus,
    ) -> Result<Vec<WorkflowRun>, SendableError> {
        self.client
            .fetch_workflow_runs_by_status(status)
            .await
            .map_err(|err| -> SendableError { Box::new(err) })
    }

    pub async fn update_workflow_run(
        &self,
        workflow_run_id: i64,
        status: WorkflowStatus,
        active_node_id: Option<String>,
        state: Option<Value>,
        message: Option<String>,
    ) -> Result<(), SendableError> {
        self.client
            .update_workflow_run(workflow_run_id, status, active_node_id, state, message)
            .await
            .map_err(|err| -> SendableError { Box::new(err) })?;
        Ok(())
    }

    pub async fn fetch_workflow_run(
        &self,
        workflow_run_id: i64,
    ) -> Result<(WorkflowRun, Vec<WorkflowNodeRun>), SendableError> {
        self.client
            .fetch_workflow_run(workflow_run_id)
            .await
            .map_err(|err| -> SendableError { Box::new(err) })
    }

    pub async fn create_workflow_node_run(
        &self,
        workflow_run_id: i64,
        node_id: &str,
        parameters: Value,
    ) -> Result<WorkflowNodeRun, SendableError> {
        self.client
            .create_workflow_node_run(workflow_run_id, node_id, parameters)
            .await
            .map_err(|err| -> SendableError { Box::new(err) })
    }

    pub async fn update_workflow_node_run(
        &self,
        node_run_id: i64,
        status: WorkflowStatus,
        task_run_id: Option<i64>,
        attempt: Option<i64>,
        parameters: Option<Value>,
        output_json: Option<Value>,
        state: Option<Value>,
        transition_reason: Option<String>,
        message: Option<String>,
    ) -> Result<(), SendableError> {
        self.client
            .update_workflow_node_run(
                node_run_id,
                status,
                task_run_id,
                attempt,
                parameters,
                output_json,
                state,
                transition_reason,
                message,
            )
            .await
            .map_err(|err| -> SendableError { Box::new(err) })?;
        Ok(())
    }

    pub async fn create_automation_record(
        &self,
        path: &str,
        record: Value,
    ) -> Result<Value, SendableError> {
        self.client
            .create_automation_record(path, record)
            .await
            .map_err(|err| -> SendableError { Box::new(err) })
    }

    pub async fn fetch_idempotency_key(
        &self,
        scope: &str,
        key: &str,
    ) -> Result<Option<Value>, SendableError> {
        self.client
            .fetch_idempotency_key(scope, key)
            .await
            .map_err(|err| -> SendableError { Box::new(err) })
    }

    pub async fn put_idempotency_key(
        &self,
        scope: &str,
        key: &str,
        result: Value,
    ) -> Result<Value, SendableError> {
        self.client
            .put_idempotency_key(scope, key, result)
            .await
            .map_err(|err| -> SendableError { Box::new(err) })
    }
}
