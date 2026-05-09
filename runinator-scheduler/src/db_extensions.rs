use chrono::{DateTime, Utc};
use croner::Cron;
use runinator_models::{core::ScheduledTask, errors::SendableError};

use crate::api::SchedulerApi;

pub(crate) async fn set_next_execution_with_cron_statement(
    api: &SchedulerApi,
    task: &mut ScheduledTask,
) -> Result<(), SendableError> {
    let now = Utc::now();
    task.next_execution = Some(next_execution_for_cron(&task.cron_schedule, now)?);
    api.update_task(task).await
}

pub(crate) async fn set_initial_execution(
    api: &SchedulerApi,
    task: &mut ScheduledTask,
) -> Result<(), SendableError> {
    set_next_execution_with_cron_statement(api, task).await
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
