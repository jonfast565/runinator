use std::time::Duration;

use runinator_models::errors::SendableError;
pub use runinator_utilities::data_export::TableData;

pub mod postgres;

pub trait DatabaseConnector: Send + Sync {
    fn execute_query(&self, sql: &str, timeout: Duration) -> Result<TableData, SendableError>;
}
