use std::future::Future;

use chrono::{DateTime, Utc};
use runinator_models::{
    core::{ScheduledTask, TaskRun},
    errors::SendableError,
};

// doing something like this would make the code look more readable:
// but alas Rust kills us every time
// pub type DatabaseAsyncReturn<T> = impl Future<Output = Result<T, SendableError>> + Send;

// NOTE: Ensure anything that implements this trait cannot contain a reference
// otherwise, this is breaking major rules
pub trait DatabaseImpl: Send + Sync + 'static {
    fn run_init_scripts(
        &self,
        paths: &Vec<String>,
    ) -> impl Future<Output = Result<(), SendableError>> + Send;
    fn upsert_task(
        &self,
        task: &ScheduledTask,
    ) -> impl Future<Output = Result<(), SendableError>> + Send;
    fn delete_task(&self, task_id: i64) -> impl Future<Output = Result<(), SendableError>> + Send;
    fn fetch_all_tasks(
        &self,
    ) -> impl Future<Output = Result<Vec<ScheduledTask>, SendableError>> + Send;
    fn fetch_task_runs(
        &self,
        start: i64,
        end: i64,
    ) -> impl Future<Output = Result<Vec<TaskRun>, SendableError>> + Send;
    fn update_task_next_execution(
        &self,
        task: &ScheduledTask,
    ) -> impl Future<Output = Result<(), SendableError>> + Send;
    fn log_task_run(
        &self,
        task_id: i64,
        start_time: DateTime<Utc>,
        duration_ms: i64,
    ) -> impl Future<Output = Result<(), SendableError>> + Send;
    fn request_immediate_run(
        &self,
        task_id: i64,
    ) -> impl Future<Output = Result<(), SendableError>> + Send;

    fn clear_immediate_run(
        &self,
        task_id: i64,
    ) -> impl Future<Output = Result<(), SendableError>> + Send;
}
