use std::sync::Arc;

use interfaces::DatabaseImpl;
use log::info;
use runinator_models::errors::SendableError;

pub mod backend;
mod common;
pub mod errors;
pub mod interfaces;
mod mappers;
pub mod mysql;
mod operations;
pub mod postgres;
mod queries;
pub mod sqlite;

pub async fn initialize_database(pool: &Arc<impl DatabaseImpl>) -> Result<(), SendableError> {
    info!("Run init scripts");
    let scripts: Vec<String> = Vec::new();
    pool.run_init_scripts(&scripts).await?;
    Ok(())
}
