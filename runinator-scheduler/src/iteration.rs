use chrono::Utc;
use log::debug;
use runinator_broker::{Broker, BrokerError, BrokerMessage};
use runinator_comm::ActionCommand;
use runinator_models::{
    errors::{RuntimeError, SendableError},
    workflows::{WorkflowAction, WorkflowNodeRun},
};
use serde_json::Value;
use uuid::Uuid;

use crate::{api::SchedulerApi, config::Config};

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
    broker: &dyn Broker,
    workflow_run_id: i64,
    node_run: &WorkflowNodeRun,
    action: &WorkflowAction,
    parameters: Value,
    dedupe_key: String,
) -> Result<(), SendableError> {
    let command = ActionCommand {
        command_id: Uuid::new_v4(),
        workflow_run_id,
        workflow_node_run_id: node_run.id,
        node_id: node_run.node_id.clone(),
        action: action.clone(),
        attempt: node_run.attempt + 1,
        parameters,
    };
    let message = BrokerMessage {
        command,
        dedupe_key: Some(dedupe_key),
        enqueued_at: Utc::now(),
    };
    broker
        .publish(message)
        .await
        .map_err(|err| broker_error("enqueue", err))
}

pub fn broker_error(context: &'static str, err: BrokerError) -> SendableError {
    Box::new(RuntimeError::new(
        format!("broker.{}", context),
        err.to_string(),
    ))
}
