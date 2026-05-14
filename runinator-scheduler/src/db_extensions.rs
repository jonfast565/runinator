use chrono::{DateTime, Utc};
use croner::Cron;
use runinator_models::{errors::SendableError, workflows::WorkflowTrigger};

use crate::api::SchedulerApi;

pub(crate) async fn set_next_execution_with_cron_statement(
    api: &SchedulerApi,
    trigger: &mut WorkflowTrigger,
) -> Result<(), SendableError> {
    let now = Utc::now();
    let cron_schedule = trigger
        .configuration
        .get("cron")
        .and_then(serde_json::Value::as_str)
        .unwrap_or_default();
    trigger.next_execution = Some(next_execution_for_cron(cron_schedule, now)?);
    let Some(trigger_id) = trigger.id else {
        return Ok(());
    };
    api.update_workflow_trigger_next_execution(trigger_id, trigger.next_execution)
        .await
}

pub(crate) async fn set_initial_execution(
    api: &SchedulerApi,
    trigger: &mut WorkflowTrigger,
) -> Result<(), SendableError> {
    set_next_execution_with_cron_statement(api, trigger).await
}

pub(crate) fn next_execution_for_cron(
    cron_schedule: &str,
    now: DateTime<Utc>,
) -> Result<DateTime<Utc>, SendableError> {
    let cron = cron_schedule
        .parse::<Cron>()
        .map_err(|err| -> SendableError { Box::new(err) })?;
    cron.find_next_occurrence(&now, false)
        .map_err(|err| -> SendableError { Box::new(err) })
}
