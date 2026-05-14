use chrono::{DateTime, Utc};
use log::{debug, error};
use runinator_broker::{Broker, BrokerError, BrokerMessage};
use runinator_comm::ActionCommand;
use runinator_models::{
    errors::{RuntimeError, SendableError},
    workflows::{WorkflowAction, WorkflowNodeRun, WorkflowTrigger},
};
use serde_json::Value;
use uuid::Uuid;

use crate::{api::SchedulerApi, config::Config, db_extensions};

pub async fn run_scheduler_iteration(
    _broker: &dyn Broker,
    api: &SchedulerApi,
    _config: &Config,
) -> Result<(), SendableError> {
    let mut triggers = api.fetch_due_workflow_triggers().await?;
    debug!(
        "Scheduler evaluating {} due workflow trigger(s)",
        triggers.len()
    );

    for trigger in &mut triggers {
        if trigger.next_execution.is_none() {
            db_extensions::set_initial_execution(api, trigger).await?;
            continue;
        }

        if is_trigger_in_blackout(trigger, Utc::now()) {
            if let (Some(trigger_id), Some(end)) = (trigger.id, trigger.blackout_end) {
                trigger.next_execution = Some(end);
                api.update_workflow_trigger_next_execution(trigger_id, trigger.next_execution)
                    .await?;
            }
            continue;
        }

        let workflow_run = match api
            .create_workflow_run(trigger.workflow_id, trigger_parameters(trigger))
            .await
        {
            Ok(run) => run,
            Err(err) => {
                error!(
                    "Failed creating workflow run for trigger {:?}: {}",
                    trigger.id, err
                );
                continue;
            }
        };
        debug!(
            "Workflow trigger {:?} queued workflow run {}",
            trigger.id, workflow_run.id
        );

        if let Err(err) = db_extensions::set_next_execution_with_cron_statement(api, trigger).await
        {
            error!(
                "Failed updating next execution for workflow trigger {:?}: {}",
                trigger.id, err
            );
        }
    }

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

fn is_trigger_in_blackout(trigger: &WorkflowTrigger, now: DateTime<Utc>) -> bool {
    if let (Some(start), Some(end)) = (trigger.blackout_start, trigger.blackout_end) {
        return now >= start && now <= end;
    }
    false
}

fn trigger_parameters(trigger: &WorkflowTrigger) -> Value {
    trigger
        .configuration
        .get("parameters")
        .cloned()
        .unwrap_or_else(|| Value::Object(Default::default()))
}
