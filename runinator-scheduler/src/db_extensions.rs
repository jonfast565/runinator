use chrono::Utc;
use croner::Cron;
use runinator_models::{core::ScheduledTask, errors::SendableError};

use crate::api::SchedulerApi;

pub(crate) async fn set_next_execution_with_cron_statement(
    api: &SchedulerApi,
    task: &mut ScheduledTask,
) -> Result<(), SendableError> {
    let cron = Cron::new(task.cron_schedule.as_str())
        .parse()
        .map_err(|err| -> SendableError { Box::new(err) })?;
    let now = Utc::now();
    let next = cron
        .find_next_occurrence(&now, false)
        .map_err(|err| -> SendableError { Box::new(err) })?;
    task.next_execution = Some(next);
    api.update_task(task).await
}

pub(crate) async fn set_initial_execution(
    api: &SchedulerApi,
    task: &mut ScheduledTask,
) -> Result<(), SendableError> {
    task.next_execution = Some(Utc::now());
    api.update_task(task).await
}
