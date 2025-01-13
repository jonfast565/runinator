use runinator_database::interfaces::DatabaseImpl;
use runinator_models::{core::{ScheduledTask, TaskRun}, web::TaskResponse};

pub async fn add_task<T: DatabaseImpl>(
    db: &T,
    scheduled_task: &ScheduledTask
) -> Result<TaskResponse, Box<dyn std::error::Error>> {
    db.upsert_task(scheduled_task).await?;
    Ok(TaskResponse {
        success: true,
        message: "Task added successfully".to_string(),
    })
}

pub async fn update_task<T: DatabaseImpl>(
    db: &T,
    scheduled_task: &ScheduledTask
) -> Result<TaskResponse, Box<dyn std::error::Error>> {
    db.upsert_task(scheduled_task).await?;
    Ok(TaskResponse {
        success: true,
        message: "Task updated successfully".to_string(),
    })
}

pub async fn delete_task<T: DatabaseImpl>(
    db: &T,
    task_id: i64
) -> Result<TaskResponse, Box<dyn std::error::Error>> {
    db.delete_task(task_id).await?;
    Ok(TaskResponse {
        success: true,
        message: format!("Task with ID {} deleted successfully", task_id),
    })
}

pub async fn fetch_tasks<T: DatabaseImpl>(
    db: &T
) -> Result<Vec<ScheduledTask>, Box<dyn std::error::Error>> {
    let result = db.fetch_all_tasks().await?;
    Ok(result)
}

pub async fn fetch_task_runs<T: DatabaseImpl>(
    db: &T,
    start: i64,
    end: i64
) -> Result<Vec<TaskRun>, Box<dyn std::error::Error>> {
    let result = db.fetch_task_runs(start, end).await?;
    Ok(result)
}