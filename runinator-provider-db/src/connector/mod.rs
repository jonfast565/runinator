use std::sync::Arc;
use std::time::Duration;

use runinator_models::errors::SendableError;
use serde::Deserialize;
use serde_json::{Map, Value};
use tokio::runtime::Runtime;

use crate::engine::Engine;
use crate::rowset::{ExecOutcome, RowSet, StepOutcome, TableInfo};
use crate::statement::StatementSpec;

#[cfg(any(feature = "postgres", feature = "mariadb", feature = "sqlite"))]
pub mod sql;
#[cfg(any(feature = "postgres", feature = "mariadb", feature = "sqlite"))]
pub(crate) mod timeout;

/// what `db.provision` should ensure exists before the workflow touches the database.
#[cfg_attr(
    not(any(feature = "postgres", feature = "mariadb", feature = "sqlite")),
    allow(dead_code)
)]

/// what a seed step inserts. `on_conflict` keeps re-running a provision step idempotent.
#[cfg_attr(
    not(any(feature = "postgres", feature = "mariadb", feature = "sqlite")),
    allow(dead_code)
)]
#[derive(Clone, Copy, Debug, Default, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum OnConflict {
    #[default]
    Error,
    Ignore,
}

/// the operations every engine must support. deliberately synchronous: the async drivers are
/// driven by a runtime each connector owns, which keeps this trait `dyn`-compatible and matches
/// the synchronous `Provider` entry point.

/// build the connector for an engine. the runtime is shared so a single provider call does not
/// stand up more than one reactor.
#[allow(unreachable_patterns)]
pub fn connector_for(
    engine: Engine,
    _connection: &str,
    _runtime: Arc<Runtime>,
) -> Result<Box<dyn DatabaseConnector>, SendableError> {
    match engine {
        #[cfg(feature = "sqlite")]
        Engine::Sqlite => Ok(Box::new(sql::SqlConnector::new(
            Engine::Sqlite,
            _connection,
            _runtime,
        )?)),
        #[cfg(feature = "postgres")]
        Engine::Postgres => Ok(Box::new(sql::SqlConnector::new(
            Engine::Postgres,
            _connection,
            _runtime,
        )?)),
        #[cfg(feature = "mariadb")]
        Engine::Mariadb => Ok(Box::new(sql::SqlConnector::new(
            Engine::Mariadb,
            _connection,
            _runtime,
        )?)),
        other => Err(crate::errors::UNSUPPORTED_ENGINE
            .error(format!("{} is not enabled in this build", other.as_str()))),
    }
}

mod provision_spec;
pub use provision_spec::ProvisionSpec;

mod seed_spec;
pub use seed_spec::SeedSpec;

mod database_connector;
pub use database_connector::DatabaseConnector;
