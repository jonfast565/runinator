use runinator_database::interfaces::DatabaseImpl;

pub async fn initialize_database<T: DatabaseImpl>(
    db: &T
) -> Result<(), Box<dyn std::error::Error>> {
    db.create_scheduled_tasks_table().await?;
    db.create_task_runs_table().await?;
    Ok(())
}