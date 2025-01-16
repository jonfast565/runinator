use std::sync::Arc;

use chrono::Utc;
use croner::Cron;
use runinator_database::interfaces::DatabaseImpl;
use runinator_models::{core::ScheduledTask, errors::SendableError};

pub(crate) async fn initialize_database(
    pool: &Arc<impl DatabaseImpl>,
) -> Result<(), SendableError> {
    let file_vec = [
        "./scripts/table_init.sql".to_string(),
        "./scripts/init.sql".to_string(),
    ]
    .to_vec();
    pool.run_init_scripts(&file_vec).await?;
    Ok(())
}

pub(crate) async fn set_next_execution_with_cron_statement(
    pool: &Arc<impl DatabaseImpl>,
    task: &ScheduledTask,
) -> Result<(), SendableError> {
    let mut task_next_execution = task.clone();
    let schedule_str = task_next_execution.cron_schedule.as_str();
    let cron = Cron::new(schedule_str)
        .parse()
        .expect("Couldn't parse cron string");
    let now = Utc::now();
    let next_upcoming = cron.find_next_occurrence(&now, false).unwrap();
    task_next_execution.next_execution = Some(next_upcoming);
    pool.update_task_next_execution(&task_next_execution)
        .await?;
    Ok(())
}

pub(crate) async fn set_initial_execution(
    pool: &Arc<impl DatabaseImpl>,
    task: &ScheduledTask,
) -> Result<(), SendableError> {
    let mut task_clone = task.clone();
    task_clone.next_execution = Some(Utc::now());
    pool.update_task_next_execution(&task_clone).await?;
    Ok(())
}
