use std::future::Future;

use chrono::{DateTime, Utc};
use runinator_models::core::{ScheduledTask, TaskRun};

// NOTE: Ensure anything that implements this trait cannot contain a reference
// otherwise, this is breaking major rules
pub trait DatabaseImpl : Send + Sync + 'static {
    fn run_init_scripts(&self, paths: &Vec<String>) -> impl Future<Output = Result<(), Box<dyn std::error::Error>>> + Send;
    fn create_scheduled_tasks_table(&self) -> impl Future<Output = Result<(), Box<dyn std::error::Error>>> + Send;
    fn create_task_runs_table(&self) -> impl Future<Output = Result<(), Box<dyn std::error::Error>>> + Send;
    fn upsert_task(&self, task: &ScheduledTask) -> impl Future<Output = Result<(), Box<dyn std::error::Error>>> + Send;
    fn delete_task(&self, task_id: i64) -> impl Future<Output = Result<(), Box<dyn std::error::Error>>> + Send;
    fn fetch_all_tasks(&self) -> impl Future<Output = Result<Vec<ScheduledTask>, Box<dyn std::error::Error>>> + Send;
    fn fetch_task_runs(&self, start: i64, end: i64) -> impl Future<Output = Result<Vec<TaskRun>, Box<dyn std::error::Error>>> + Send;
    fn update_task_next_execution(&self, task: &ScheduledTask) -> impl Future<Output = Result<(), Box<dyn std::error::Error>>> + Send;
    fn log_task_run(
        &self,
        task_name: &str,
        start_time: DateTime<Utc>,
        duration_ms: i64,
    ) -> impl Future<Output = Result<(), Box<dyn std::error::Error>>> + Send;
}
