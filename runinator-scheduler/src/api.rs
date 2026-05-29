use std::time::Duration;

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use runinator_api::{AsyncApiClient, ServiceLocator, StaticLocator};
use runinator_comm::{ActionCommand, ActionDispatchRecord};
use runinator_models::value::Value;
use runinator_models::{
    errors::SendableError,
    providers::ProviderMetadata,
    runs::{RunStatus, RunSummary},
    workflows::{
        WorkflowDefinition, WorkflowNodeRun, WorkflowRun, WorkflowStatus, WorkflowTrigger,
    },
};

use crate::nodes::RunState;
use crate::worker_comm::WorkerManager;

#[async_trait]
pub trait WorkflowSchedulerApi: Send + Sync {
    async fn fetch_workflow(&self, workflow_id: i64) -> Result<WorkflowDefinition, SendableError>;

    async fn fetch_workflow_by_name(&self, name: &str)
    -> Result<WorkflowDefinition, SendableError>;

    async fn fetch_providers(&self) -> Result<Vec<ProviderMetadata>, SendableError>;

    async fn create_workflow_run(
        &self,
        workflow_id: i64,
        parameters: Value,
    ) -> Result<WorkflowRun, SendableError>;

    async fn create_named_workflow_run(
        &self,
        workflow_id: i64,
        parameters: Value,
        name: String,
    ) -> Result<WorkflowRun, SendableError>;

    async fn fetch_due_workflow_triggers(&self) -> Result<Vec<WorkflowTrigger>, SendableError>;

    async fn claim_due_workflow_trigger_firings(
        &self,
        scheduler_id: &str,
        limit: i64,
    ) -> Result<Vec<WorkflowRun>, SendableError>;

    async fn update_workflow_trigger_next_execution(
        &self,
        trigger_id: i64,
        next_execution: Option<DateTime<Utc>>,
    ) -> Result<(), SendableError>;

    async fn fetch_workflow_runs_by_status(
        &self,
        status: WorkflowStatus,
    ) -> Result<Vec<WorkflowRun>, SendableError>;

    async fn claim_workflow_runs_for_scheduler(
        &self,
        scheduler_id: &str,
        statuses: &[WorkflowStatus],
        lease_until: DateTime<Utc>,
        limit: i64,
    ) -> Result<Vec<WorkflowRun>, SendableError>;

    async fn renew_workflow_run_claim(
        &self,
        workflow_run_id: i64,
        scheduler_id: &str,
        lease_until: DateTime<Utc>,
    ) -> Result<bool, SendableError>;

    async fn release_workflow_run_claim(
        &self,
        workflow_run_id: i64,
        scheduler_id: &str,
    ) -> Result<(), SendableError>;

    async fn fetch_workflow_runs_by_name(
        &self,
        name: &str,
        open_only: bool,
    ) -> Result<Vec<WorkflowRun>, SendableError>;

    async fn update_workflow_run(
        &self,
        workflow_run_id: i64,
        status: WorkflowStatus,
        active_node_id: Option<String>,
        state: Option<Value>,
        message: Option<String>,
    ) -> Result<(), SendableError>;

    async fn set_workflow_run_name(
        &self,
        workflow_run_id: i64,
        name: Option<String>,
    ) -> Result<(), SendableError>;

    async fn fetch_workflow_run(
        &self,
        workflow_run_id: i64,
    ) -> Result<(WorkflowRun, Vec<WorkflowNodeRun>), SendableError>;

    async fn create_workflow_node_run(
        &self,
        workflow_run_id: i64,
        node_id: &str,
        parameters: Value,
    ) -> Result<WorkflowNodeRun, SendableError>;

    #[allow(clippy::too_many_arguments)]
    async fn update_workflow_node_run(
        &self,
        node_run_id: i64,
        status: WorkflowStatus,
        attempt: Option<i64>,
        parameters: Option<Value>,
        output_json: Option<Value>,
        state: Option<Value>,
        transition_reason: Option<String>,
        message: Option<String>,
    ) -> Result<(), SendableError>;

    async fn create_automation_record(
        &self,
        path: &str,
        record: Value,
    ) -> Result<Value, SendableError>;

    async fn fetch_idempotency_key(
        &self,
        scope: &str,
        key: &str,
    ) -> Result<Option<Value>, SendableError>;

    async fn put_idempotency_key(
        &self,
        scope: &str,
        key: &str,
        result: Value,
    ) -> Result<Value, SendableError>;

    async fn enqueue_action_dispatch(
        &self,
        dedupe_key: &str,
        command: &ActionCommand,
    ) -> Result<ActionDispatchRecord, SendableError>;

    async fn fetch_pending_action_dispatches(
        &self,
        limit: i64,
    ) -> Result<Vec<ActionDispatchRecord>, SendableError>;

    async fn mark_action_dispatch_published(&self, dispatch_id: i64) -> Result<(), SendableError>;

    async fn mark_action_dispatch_failed(
        &self,
        dispatch_id: i64,
        error: &str,
    ) -> Result<(), SendableError>;
}

#[derive(Clone)]
pub struct SchedulerApi {
    client: AsyncApiClient<SchedulerServiceLocator>,
}

#[derive(Clone)]
pub enum SchedulerServiceLocator {
    Static(StaticLocator),
    Gossip(WorkerManager),
}

#[async_trait]
impl ServiceLocator for SchedulerServiceLocator {
    type Error = std::convert::Infallible;

    async fn wait_for_service_url(&self) -> Result<String, Self::Error> {
        match self {
            Self::Static(locator) => locator.wait_for_service_url().await,
            Self::Gossip(locator) => Ok(locator.service_registry().wait_for_service_url().await),
        }
    }
}

impl SchedulerApi {
    pub fn new(locator: SchedulerServiceLocator, timeout: Duration) -> Result<Self, SendableError> {
        let http_client = reqwest::Client::builder()
            .timeout(timeout)
            .build()
            .map_err(|err| -> SendableError { Box::new(err) })?;

        Ok(Self {
            client: AsyncApiClient::with_client(locator, http_client),
        })
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

    pub async fn fetch_workflow_by_name(
        &self,
        name: &str,
    ) -> Result<WorkflowDefinition, SendableError> {
        self.client
            .fetch_workflow_by_name(name)
            .await
            .map_err(|err| -> SendableError { Box::new(err) })
    }

    pub async fn fetch_providers(&self) -> Result<Vec<ProviderMetadata>, SendableError> {
        self.client
            .fetch_providers()
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

    pub async fn create_named_workflow_run(
        &self,
        workflow_id: i64,
        parameters: Value,
        name: String,
    ) -> Result<WorkflowRun, SendableError> {
        self.client
            .create_named_workflow_run(workflow_id, parameters, name)
            .await
            .map_err(|err| -> SendableError { Box::new(err) })
    }

    pub async fn fetch_due_workflow_triggers(&self) -> Result<Vec<WorkflowTrigger>, SendableError> {
        self.client
            .fetch_due_workflow_triggers()
            .await
            .map_err(|err| -> SendableError { Box::new(err) })
    }

    pub async fn claim_due_workflow_trigger_firings(
        &self,
        scheduler_id: &str,
        limit: i64,
    ) -> Result<Vec<WorkflowRun>, SendableError> {
        self.client
            .claim_due_workflow_trigger_firings(scheduler_id, limit)
            .await
            .map_err(|err| -> SendableError { Box::new(err) })
    }

    pub async fn update_workflow_trigger_next_execution(
        &self,
        trigger_id: i64,
        next_execution: Option<DateTime<Utc>>,
    ) -> Result<(), SendableError> {
        let mut trigger = self
            .client
            .fetch_workflow_trigger(trigger_id)
            .await
            .map_err(|err| -> SendableError { Box::new(err) })?;
        trigger.next_execution = next_execution;
        self.client
            .upsert_workflow_trigger(&trigger)
            .await
            .map_err(|err| -> SendableError { Box::new(err) })?;
        Ok(())
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

    pub async fn claim_workflow_runs_for_scheduler(
        &self,
        scheduler_id: &str,
        statuses: &[WorkflowStatus],
        lease_until: DateTime<Utc>,
        limit: i64,
    ) -> Result<Vec<WorkflowRun>, SendableError> {
        self.client
            .claim_workflow_runs_for_scheduler(scheduler_id, statuses, lease_until, limit)
            .await
            .map_err(|err| -> SendableError { Box::new(err) })
    }

    pub async fn renew_workflow_run_claim(
        &self,
        workflow_run_id: i64,
        scheduler_id: &str,
        lease_until: DateTime<Utc>,
    ) -> Result<bool, SendableError> {
        match self
            .client
            .renew_workflow_run_claim(workflow_run_id, scheduler_id, lease_until)
            .await
        {
            Ok(_) => Ok(true),
            Err(err) => {
                if err.to_string().contains("404") {
                    Ok(false)
                } else {
                    Err(Box::new(err))
                }
            }
        }
    }

    pub async fn release_workflow_run_claim(
        &self,
        workflow_run_id: i64,
        scheduler_id: &str,
    ) -> Result<(), SendableError> {
        self.client
            .release_workflow_run_claim(workflow_run_id, scheduler_id)
            .await
            .map_err(|err| -> SendableError { Box::new(err) })?;
        Ok(())
    }

    pub async fn fetch_workflow_runs_by_name(
        &self,
        name: &str,
        open_only: bool,
    ) -> Result<Vec<WorkflowRun>, SendableError> {
        self.client
            .fetch_workflow_runs_by_name(name, open_only)
            .await
            .map_err(|err| -> SendableError { Box::new(err) })
    }

    pub async fn update_workflow_run(
        &self,
        workflow_run_id: i64,
        status: WorkflowStatus,
        active_node_id: Option<String>,
        mut state: Option<Value>,
        message: Option<String>,
    ) -> Result<(), SendableError> {
        if let Some(next_state) = state.as_mut()
            && let Ok((run, _)) = self.client.fetch_workflow_run(workflow_run_id).await
        {
            let existing = RunState::from_value(&run.state);
            if let Some(debug) = existing.debug() {
                let mut next = RunState::from_value(next_state);
                // carry the prior debug frame forward only when the new state omits one.
                if next.debug().is_none() {
                    next.set_debug(debug.clone());
                    *next_state = next.into_value()?;
                }
            }
        }
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

    pub async fn set_workflow_run_name(
        &self,
        workflow_run_id: i64,
        name: Option<String>,
    ) -> Result<(), SendableError> {
        self.client
            .rename_workflow_run(workflow_run_id, name)
            .await
            .map_err(|err| -> SendableError { Box::new(err) })?;
        Ok(())
    }

    pub async fn pause_workflow_run(&self, workflow_run_id: i64) -> Result<(), SendableError> {
        self.client
            .pause_workflow_run(workflow_run_id)
            .await
            .map_err(|err| -> SendableError { Box::new(err) })?;
        Ok(())
    }

    pub async fn resume_workflow_run(&self, workflow_run_id: i64) -> Result<(), SendableError> {
        self.client
            .resume_workflow_run(workflow_run_id)
            .await
            .map_err(|err| -> SendableError { Box::new(err) })?;
        Ok(())
    }

    pub async fn cancel_workflow_run(&self, workflow_run_id: i64) -> Result<(), SendableError> {
        self.client
            .cancel_workflow_run(workflow_run_id)
            .await
            .map_err(|err| -> SendableError { Box::new(err) })?;
        Ok(())
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
    ) -> Result<(), SendableError> {
        self.client
            .update_workflow_node_run(
                node_run_id,
                status,
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

    pub async fn enqueue_action_dispatch(
        &self,
        dedupe_key: &str,
        command: &ActionCommand,
    ) -> Result<ActionDispatchRecord, SendableError> {
        self.client
            .enqueue_action_dispatch(dedupe_key, command)
            .await
            .map_err(|err| -> SendableError { Box::new(err) })
    }

    pub async fn fetch_pending_action_dispatches(
        &self,
        limit: i64,
    ) -> Result<Vec<ActionDispatchRecord>, SendableError> {
        self.client
            .fetch_pending_action_dispatches(limit)
            .await
            .map_err(|err| -> SendableError { Box::new(err) })
    }

    pub async fn mark_action_dispatch_published(
        &self,
        dispatch_id: i64,
    ) -> Result<(), SendableError> {
        self.client
            .mark_action_dispatch_published(dispatch_id)
            .await
            .map_err(|err| -> SendableError { Box::new(err) })?;
        Ok(())
    }

    pub async fn mark_action_dispatch_failed(
        &self,
        dispatch_id: i64,
        error: &str,
    ) -> Result<(), SendableError> {
        self.client
            .mark_action_dispatch_failed(dispatch_id, error)
            .await
            .map_err(|err| -> SendableError { Box::new(err) })?;
        Ok(())
    }
}

#[async_trait]
impl WorkflowSchedulerApi for SchedulerApi {
    async fn fetch_workflow(&self, workflow_id: i64) -> Result<WorkflowDefinition, SendableError> {
        SchedulerApi::fetch_workflow(self, workflow_id).await
    }

    async fn fetch_workflow_by_name(
        &self,
        name: &str,
    ) -> Result<WorkflowDefinition, SendableError> {
        SchedulerApi::fetch_workflow_by_name(self, name).await
    }

    async fn fetch_providers(&self) -> Result<Vec<ProviderMetadata>, SendableError> {
        SchedulerApi::fetch_providers(self).await
    }

    async fn create_workflow_run(
        &self,
        workflow_id: i64,
        parameters: Value,
    ) -> Result<WorkflowRun, SendableError> {
        SchedulerApi::create_workflow_run(self, workflow_id, parameters).await
    }

    async fn create_named_workflow_run(
        &self,
        workflow_id: i64,
        parameters: Value,
        name: String,
    ) -> Result<WorkflowRun, SendableError> {
        SchedulerApi::create_named_workflow_run(self, workflow_id, parameters, name).await
    }

    async fn fetch_due_workflow_triggers(&self) -> Result<Vec<WorkflowTrigger>, SendableError> {
        SchedulerApi::fetch_due_workflow_triggers(self).await
    }

    async fn claim_due_workflow_trigger_firings(
        &self,
        scheduler_id: &str,
        limit: i64,
    ) -> Result<Vec<WorkflowRun>, SendableError> {
        SchedulerApi::claim_due_workflow_trigger_firings(self, scheduler_id, limit).await
    }

    async fn update_workflow_trigger_next_execution(
        &self,
        trigger_id: i64,
        next_execution: Option<DateTime<Utc>>,
    ) -> Result<(), SendableError> {
        SchedulerApi::update_workflow_trigger_next_execution(self, trigger_id, next_execution).await
    }

    async fn fetch_workflow_runs_by_status(
        &self,
        status: WorkflowStatus,
    ) -> Result<Vec<WorkflowRun>, SendableError> {
        SchedulerApi::fetch_workflow_runs_by_status(self, status).await
    }

    async fn claim_workflow_runs_for_scheduler(
        &self,
        scheduler_id: &str,
        statuses: &[WorkflowStatus],
        lease_until: DateTime<Utc>,
        limit: i64,
    ) -> Result<Vec<WorkflowRun>, SendableError> {
        SchedulerApi::claim_workflow_runs_for_scheduler(
            self,
            scheduler_id,
            statuses,
            lease_until,
            limit,
        )
        .await
    }

    async fn renew_workflow_run_claim(
        &self,
        workflow_run_id: i64,
        scheduler_id: &str,
        lease_until: DateTime<Utc>,
    ) -> Result<bool, SendableError> {
        SchedulerApi::renew_workflow_run_claim(self, workflow_run_id, scheduler_id, lease_until)
            .await
    }

    async fn release_workflow_run_claim(
        &self,
        workflow_run_id: i64,
        scheduler_id: &str,
    ) -> Result<(), SendableError> {
        SchedulerApi::release_workflow_run_claim(self, workflow_run_id, scheduler_id).await
    }

    async fn fetch_workflow_runs_by_name(
        &self,
        name: &str,
        open_only: bool,
    ) -> Result<Vec<WorkflowRun>, SendableError> {
        SchedulerApi::fetch_workflow_runs_by_name(self, name, open_only).await
    }

    async fn update_workflow_run(
        &self,
        workflow_run_id: i64,
        status: WorkflowStatus,
        active_node_id: Option<String>,
        state: Option<Value>,
        message: Option<String>,
    ) -> Result<(), SendableError> {
        SchedulerApi::update_workflow_run(
            self,
            workflow_run_id,
            status,
            active_node_id,
            state,
            message,
        )
        .await
    }

    async fn set_workflow_run_name(
        &self,
        workflow_run_id: i64,
        name: Option<String>,
    ) -> Result<(), SendableError> {
        SchedulerApi::set_workflow_run_name(self, workflow_run_id, name).await
    }

    async fn fetch_workflow_run(
        &self,
        workflow_run_id: i64,
    ) -> Result<(WorkflowRun, Vec<WorkflowNodeRun>), SendableError> {
        SchedulerApi::fetch_workflow_run(self, workflow_run_id).await
    }

    async fn create_workflow_node_run(
        &self,
        workflow_run_id: i64,
        node_id: &str,
        parameters: Value,
    ) -> Result<WorkflowNodeRun, SendableError> {
        SchedulerApi::create_workflow_node_run(self, workflow_run_id, node_id, parameters).await
    }

    async fn update_workflow_node_run(
        &self,
        node_run_id: i64,
        status: WorkflowStatus,
        attempt: Option<i64>,
        parameters: Option<Value>,
        output_json: Option<Value>,
        state: Option<Value>,
        transition_reason: Option<String>,
        message: Option<String>,
    ) -> Result<(), SendableError> {
        SchedulerApi::update_workflow_node_run(
            self,
            node_run_id,
            status,
            attempt,
            parameters,
            output_json,
            state,
            transition_reason,
            message,
        )
        .await
    }

    async fn create_automation_record(
        &self,
        path: &str,
        record: Value,
    ) -> Result<Value, SendableError> {
        SchedulerApi::create_automation_record(self, path, record).await
    }

    async fn fetch_idempotency_key(
        &self,
        scope: &str,
        key: &str,
    ) -> Result<Option<Value>, SendableError> {
        SchedulerApi::fetch_idempotency_key(self, scope, key).await
    }

    async fn put_idempotency_key(
        &self,
        scope: &str,
        key: &str,
        result: Value,
    ) -> Result<Value, SendableError> {
        SchedulerApi::put_idempotency_key(self, scope, key, result).await
    }

    async fn enqueue_action_dispatch(
        &self,
        dedupe_key: &str,
        command: &ActionCommand,
    ) -> Result<ActionDispatchRecord, SendableError> {
        SchedulerApi::enqueue_action_dispatch(self, dedupe_key, command).await
    }

    async fn fetch_pending_action_dispatches(
        &self,
        limit: i64,
    ) -> Result<Vec<ActionDispatchRecord>, SendableError> {
        SchedulerApi::fetch_pending_action_dispatches(self, limit).await
    }

    async fn mark_action_dispatch_published(&self, dispatch_id: i64) -> Result<(), SendableError> {
        SchedulerApi::mark_action_dispatch_published(self, dispatch_id).await
    }

    async fn mark_action_dispatch_failed(
        &self,
        dispatch_id: i64,
        error: &str,
    ) -> Result<(), SendableError> {
        SchedulerApi::mark_action_dispatch_failed(self, dispatch_id, error).await
    }
}
