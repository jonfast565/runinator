use chrono::{Duration, Utc};
use log::{debug, error};
use runinator_broker::{Broker, BrokerError, BrokerMessage};
use runinator_comm::ActionCommand;
use runinator_models::value::Value;
use runinator_models::{
    errors::SendableError,
    workflows::{WorkflowAction, WorkflowNodeRun},
};
use uuid::Uuid;

use crate::{
    api::{SchedulerApi, WorkflowSchedulerApi},
    config::Config,
};

pub async fn run_scheduler_iteration(
    _broker: &dyn Broker,
    api: &SchedulerApi,
    config: &Config,
) -> Result<(), SendableError> {
    let runs = api
        .claim_due_workflow_trigger_firings(&config.scheduler_id, config.scheduler_claim_limit)
        .await?;
    debug!(
        "Scheduler claimed {} due workflow trigger firing(s)",
        runs.len()
    );
    Ok(())
}

pub async fn enqueue_action_with_dedupe(
    api: &dyn WorkflowSchedulerApi,
    workflow_run_id: i64,
    node_run: &WorkflowNodeRun,
    action: &WorkflowAction,
    parameters: Value,
    dedupe_key: String,
) -> Result<(), SendableError> {
    let command = build_action_command(workflow_run_id, node_run, action, parameters);
    api.enqueue_action_dispatch(&dedupe_key, &command).await?;
    Ok(())
}

pub async fn publish_pending_action_dispatches(
    broker: &dyn Broker,
    api: &dyn WorkflowSchedulerApi,
    scheduler_id: &str,
    lease_seconds: u64,
    limit: i64,
) -> Result<(), SendableError> {
    let lease_until = Utc::now() + Duration::seconds(lease_seconds as i64);
    for dispatch in api
        .claim_pending_action_dispatches(scheduler_id, lease_until, limit)
        .await?
    {
        let dispatch_id = dispatch.id;
        let dedupe_key = dispatch.dedupe_key.clone();
        let message = BrokerMessage {
            command: dispatch.command,
            dedupe_key: Some(dedupe_key),
            enqueued_at: Utc::now(),
        };
        match broker.publish(message).await {
            Ok(()) | Err(BrokerError::Duplicate(_)) => {
                api.mark_action_dispatch_published(dispatch_id).await?;
            }
            Err(err) => {
                let message = err.to_string();
                error!(
                    "Failed publishing action dispatch {}: {}",
                    dispatch_id, message
                );
                api.mark_action_dispatch_failed(dispatch_id, &message)
                    .await?;
            }
        }
    }
    Ok(())
}

fn build_action_command(
    workflow_run_id: i64,
    node_run: &WorkflowNodeRun,
    action: &WorkflowAction,
    parameters: Value,
) -> ActionCommand {
    ActionCommand {
        command_id: Uuid::new_v4(),
        workflow_run_id,
        workflow_node_run_id: node_run.id,
        node_id: node_run.node_id.clone(),
        action: action.clone(),
        attempt: node_run.attempt + 1,
        parameters,
    }
}
