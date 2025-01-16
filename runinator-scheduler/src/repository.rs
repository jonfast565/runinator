use runinator_database::interfaces::DatabaseImpl;
use runinator_plugin::provider::Provider;
use runinator_provider_aws::AwsProvider;
use runinator_provider_sql::SqlProvider;

pub(crate) type StaticProvider = Box<dyn Provider + Send + Sync>;

pub(crate) async fn initialize_database<T: DatabaseImpl>(
    db: &T,
) -> Result<(), Box<dyn std::error::Error>> {
    db.create_scheduled_tasks_table().await?;
    db.create_task_runs_table().await?;
    Ok(())
}

pub(crate) fn get_providers() -> Vec<StaticProvider> {
    let mut result = Vec::new();
    result.push(Box::new(AwsProvider {}) as StaticProvider);
    result.push(Box::new(SqlProvider {}) as StaticProvider);
    result
}
