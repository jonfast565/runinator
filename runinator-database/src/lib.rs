use std::sync::Arc;

use interfaces::DatabaseImpl;
use log::info;
use runinator_models::errors::SendableError;

pub mod interfaces;
mod mappers;
pub mod sqlite;

pub async fn initialize_database(pool: &Arc<impl DatabaseImpl>) -> Result<(), SendableError> {
    info!("Run init scripts");
    let file_vec = [
        "./scripts/table_init.sql".to_string(),
        "./scripts/init.sql".to_string(),
    ]
    .to_vec();
    pool.run_init_scripts(&file_vec).await?;
    Ok(())
}
