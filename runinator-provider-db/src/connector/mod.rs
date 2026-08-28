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
#[derive(Debug, Default)]
pub struct ProvisionSpec {
    /// a maintenance connection used to issue `CREATE DATABASE`. postgres and mariadb cannot
    /// create a database over a connection to that same database, so without this a missing
    /// database is reported rather than created.
    pub admin_connection: Option<String>,
    /// the database name to create. derived from the connection string when omitted.
    pub database: Option<String>,
}

/// what a seed step inserts. `on_conflict` keeps re-running a provision step idempotent.
#[cfg_attr(
    not(any(feature = "postgres", feature = "mariadb", feature = "sqlite")),
    allow(dead_code)
)]
#[derive(Clone, Debug, Deserialize)]
pub struct SeedSpec {
    /// the SQL table to insert into.
    pub table: String,
    pub rows: Vec<Map<String, Value>>,
    #[serde(default)]
    pub on_conflict: OnConflict,
}

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
pub trait DatabaseConnector: Send + Sync {
    /// create the database if it is missing. returns whether it was created.
    fn ensure_database(
        &self,
        spec: &ProvisionSpec,
        timeout: Duration,
    ) -> Result<bool, SendableError>;

    fn query(&self, statement: &StatementSpec, timeout: Duration) -> Result<RowSet, SendableError>;

    fn execute(
        &self,
        statement: &StatementSpec,
        timeout: Duration,
    ) -> Result<ExecOutcome, SendableError>;

    /// run statements in order, optionally inside a single transaction.
    fn script(
        &self,
        statements: &[StatementSpec],
        transactional: bool,
        timeout: Duration,
    ) -> Result<Vec<StepOutcome>, SendableError>;

    fn seed(&self, seeds: &[SeedSpec], timeout: Duration) -> Result<u64, SendableError>;

    fn inspect(&self, timeout: Duration) -> Result<Vec<TableInfo>, SendableError>;
}

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
