pub mod execute;
pub mod inspect;
pub mod provision;
pub mod query;
pub mod script;

use std::sync::Arc;

use runinator_models::errors::SendableError;
use runinator_plugin::cancel::CancellationToken;
use serde::Deserialize;
use tokio::runtime::Runtime;

use crate::connector::{DatabaseConnector, connector_for};
use crate::engine::Engine;
use crate::errors::STATEMENT_CANCELED;

/// which projection of a row set the caller wants back.
#[derive(Clone, Copy, Debug, Default, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum Shape {
    #[default]
    Rows,
    Table,
}

impl Shape {
    pub fn as_str(&self) -> &'static str {
        match self {
            Shape::Rows => "rows",
            Shape::Table => "table",
        }
    }

    /// project a row set into the requested wire shape.
    pub fn project(&self, rows: &crate::rowset::RowSet) -> serde_json::Value {
        match self {
            Shape::Rows => rows.to_rows_json(),
            Shape::Table => rows.to_table_json(),
        }
    }
}

/// open a connector for an action request. the runtime is built once per provider call.
pub fn open(
    engine: Engine,
    connection: &str,
    runtime: Arc<Runtime>,
) -> Result<Box<dyn DatabaseConnector>, SendableError> {
    connector_for(engine, connection, runtime)
}

/// cooperative cancellation checkpoint, called between statements the way the worker expects.
pub fn check_cancelled(token: &CancellationToken) -> Result<(), SendableError> {
    if token.is_cancelled() {
        return Err(STATEMENT_CANCELED.error("database action canceled"));
    }
    Ok(())
}
