pub mod decode;
pub mod ops;

use std::str::FromStr;
use std::sync::Arc;
use std::time::Duration;

use runinator_models::errors::SendableError;
use serde_json::Value;
#[cfg(feature = "sqlite")]
use sqlx::{ConnectOptions, Connection, SqlitePool, sqlite::SqliteConnectOptions};
#[cfg(feature = "mariadb")]
use sqlx::{MySqlPool, mysql::MySqlConnectOptions};
#[cfg(feature = "postgres")]
use sqlx::{PgPool, postgres::PgConnectOptions};
use tokio::runtime::Runtime;

use crate::connector::{DatabaseConnector, OnConflict, ProvisionSpec, SeedSpec};
use crate::engine::Engine;
#[cfg(any(feature = "postgres", feature = "mariadb"))]
use crate::errors::DATABASE_MISSING;
use crate::errors::{CONNECTION_FAILED, UNSUPPORTED_ENGINE};
use crate::rowset::{ColumnSummary, ExecOutcome, RowSet, StepOutcome, TableInfo};
use crate::statement::StatementSpec;

use ops::{SqlStep, sql_returns_rows};

/// a live pool for one of the sql backends. connections are opened per provider call and closed
/// on the way out; a workflow task is not a hot path, and this keeps the desktop agent from
/// holding sockets open between runs.
enum SqlPool {
    #[cfg(feature = "postgres")]
    Postgres(PgPool),
    #[cfg(feature = "mariadb")]
    MariaDb(MySqlPool),
    #[cfg(feature = "sqlite")]
    Sqlite(SqlitePool),
}

impl SqlPool {
    async fn close(&self) {
        match self {
            #[cfg(feature = "postgres")]
            SqlPool::Postgres(pool) => pool.close().await,
            #[cfg(feature = "mariadb")]
            SqlPool::MariaDb(pool) => pool.close().await,
            #[cfg(feature = "sqlite")]
            SqlPool::Sqlite(pool) => pool.close().await,
        }
    }
}

pub struct SqlConnector {
    engine: Engine,
    connection: String,
    runtime: Arc<Runtime>,
}

impl SqlConnector {
    pub fn new(
        engine: Engine,
        connection: &str,
        runtime: Arc<Runtime>,
    ) -> Result<Self, SendableError> {
        if connection.trim().is_empty() {
            return Err(CONNECTION_FAILED.error("'connection' must not be empty"));
        }
        Ok(Self {
            engine,
            connection: connection.to_string(),
            runtime,
        })
    }

    #[allow(unreachable_patterns)]
    async fn connect(&self) -> Result<SqlPool, SendableError> {
        match self.engine {
            #[cfg(feature = "postgres")]
            Engine::Postgres => PgPool::connect(&self.connection)
                .await
                .map(SqlPool::Postgres)
                .map_err(connect_error),
            #[cfg(feature = "mariadb")]
            Engine::Mariadb => MySqlPool::connect(&self.connection)
                .await
                .map(SqlPool::MariaDb)
                .map_err(connect_error),
            #[cfg(feature = "sqlite")]
            Engine::Sqlite => {
                // never create the file as a side effect of a query; `provision` owns creation.
                let options = SqliteConnectOptions::from_str(&self.connection)
                    .map_err(connect_error)?
                    .create_if_missing(false);
                SqlitePool::connect_with(options)
                    .await
                    .map(SqlPool::Sqlite)
                    .map_err(connect_error)
            }
            other => Err(UNSUPPORTED_ENGINE
                .error(format!("{} is not enabled in this build", other.as_str()))),
        }
    }

    fn quote(&self, identifier: &str) -> String {
        // reject rather than escape: a seed target with a quote in it is a mistake, not a name.
        match self.engine {
            Engine::Mariadb => format!("`{}`", identifier.replace('`', "")),
            _ => format!("\"{}\"", identifier.replace('"', "")),
        }
    }

    fn placeholder(&self, index: usize) -> String {
        self.engine.placeholder(index)
    }
}

fn connect_error(err: sqlx::Error) -> SendableError {
    CONNECTION_FAILED.error(err.to_string())
}

/// pull SQL text and parameters out of a resolved statement.
fn sql_parts(statement: &StatementSpec) -> (&str, &[Value]) {
    match statement {
        StatementSpec::Sql { text, params, .. } => (text.as_str(), params.as_slice()),
    }
}

impl DatabaseConnector for SqlConnector {
    #[allow(unreachable_patterns)]
    fn ensure_database(
        &self,
        spec: &ProvisionSpec,
        timeout: Duration,
    ) -> Result<bool, SendableError> {
        let engine = self.engine;
        let connection = self.connection.clone();
        let admin = spec.admin_connection.clone();
        let database = spec.database.clone();
        let _ = (&admin, &database, timeout);

        self.runtime.clone().block_on(async move {
            match engine {
                #[cfg(feature = "sqlite")]
                Engine::Sqlite => ensure_sqlite(&connection).await,
                #[cfg(feature = "postgres")]
                Engine::Postgres => {
                    ensure_postgres(&connection, admin.as_deref(), database.as_deref(), timeout)
                        .await
                }
                #[cfg(feature = "mariadb")]
                Engine::Mariadb => {
                    ensure_mariadb(&connection, admin.as_deref(), database.as_deref(), timeout)
                        .await
                }
                other => Err(UNSUPPORTED_ENGINE
                    .error(format!("{} is not enabled in this build", other.as_str()))),
            }
        })
    }

    fn query(&self, statement: &StatementSpec, timeout: Duration) -> Result<RowSet, SendableError> {
        let (text, params) = sql_parts(statement);
        self.runtime.clone().block_on(async move {
            let pool = self.connect().await?;
            let result = match &pool {
                #[cfg(feature = "postgres")]
                SqlPool::Postgres(pool) => ops::pg::query(pool, text, params, timeout).await,
                #[cfg(feature = "mariadb")]
                SqlPool::MariaDb(pool) => ops::mysql::query(pool, text, params, timeout).await,
                #[cfg(feature = "sqlite")]
                SqlPool::Sqlite(pool) => ops::sqlite::query(pool, text, params, timeout).await,
            };
            pool.close().await;
            result
        })
    }

    fn execute(
        &self,
        statement: &StatementSpec,
        timeout: Duration,
    ) -> Result<ExecOutcome, SendableError> {
        let (text, params) = sql_parts(statement);
        self.runtime.clone().block_on(async move {
            let pool = self.connect().await?;
            let result = match &pool {
                #[cfg(feature = "postgres")]
                SqlPool::Postgres(pool) => ops::pg::execute(pool, text, params, timeout).await,
                #[cfg(feature = "mariadb")]
                SqlPool::MariaDb(pool) => ops::mysql::execute(pool, text, params, timeout).await,
                #[cfg(feature = "sqlite")]
                SqlPool::Sqlite(pool) => ops::sqlite::execute(pool, text, params, timeout).await,
            };
            pool.close().await;
            result
        })
    }

    fn script(
        &self,
        statements: &[StatementSpec],
        transactional: bool,
        timeout: Duration,
    ) -> Result<Vec<StepOutcome>, SendableError> {
        let steps = statements
            .iter()
            .map(|statement| {
                let (text, params) = sql_parts(statement);
                SqlStep {
                    text: text.to_string(),
                    params: params.to_vec(),
                    returns_rows: sql_returns_rows(text),
                }
            })
            .collect();

        self.run_steps(steps, transactional, timeout)
    }

    fn seed(&self, seeds: &[SeedSpec], timeout: Duration) -> Result<u64, SendableError> {
        let mut steps = Vec::new();
        for seed in seeds {
            for row in &seed.rows {
                if row.is_empty() {
                    continue;
                }
                let columns = row.keys().cloned().collect::<Vec<_>>();
                let placeholders = (0..columns.len())
                    .map(|index| self.placeholder(index))
                    .collect::<Vec<_>>()
                    .join(", ");
                let column_list = columns
                    .iter()
                    .map(|column| self.quote(column))
                    .collect::<Vec<_>>()
                    .join(", ");

                let (prefix, suffix) = match (self.engine, seed.on_conflict) {
                    (_, OnConflict::Error) => ("INSERT INTO", ""),
                    (Engine::Sqlite, OnConflict::Ignore) => ("INSERT OR IGNORE INTO", ""),
                    (Engine::Mariadb, OnConflict::Ignore) => ("INSERT IGNORE INTO", ""),
                    (Engine::Postgres, OnConflict::Ignore) => {
                        ("INSERT INTO", " ON CONFLICT DO NOTHING")
                    }
                };

                steps.push(SqlStep {
                    text: format!(
                        "{prefix} {} ({column_list}) VALUES ({placeholders}){suffix}",
                        self.quote(&seed.table)
                    ),
                    params: columns
                        .iter()
                        .map(|column| row.get(column).cloned().unwrap_or(Value::Null))
                        .collect(),
                    returns_rows: false,
                });
            }
        }

        if steps.is_empty() {
            return Ok(0);
        }

        // seeding is all-or-nothing so a failed provision does not leave half a fixture behind.
        let outcomes = self.run_steps(steps, true, timeout)?;
        Ok(outcomes
            .iter()
            .map(|outcome| match outcome {
                StepOutcome::Affected(exec) => exec.rows_affected,
                StepOutcome::Rows(rows) => rows.row_count() as u64,
            })
            .sum())
    }

    fn inspect(&self, timeout: Duration) -> Result<Vec<TableInfo>, SendableError> {
        let statement = StatementSpec::Sql {
            name: None,
            text: inspect_sql(self.engine).to_string(),
            params: Vec::new(),
        };
        let rows = self.query(&statement, timeout)?;
        Ok(group_tables(&rows))
    }
}

impl SqlConnector {
    fn run_steps(
        &self,
        steps: Vec<SqlStep>,
        transactional: bool,
        timeout: Duration,
    ) -> Result<Vec<StepOutcome>, SendableError> {
        self.runtime.clone().block_on(async move {
            let pool = self.connect().await?;
            let result = match &pool {
                #[cfg(feature = "postgres")]
                SqlPool::Postgres(pool) => {
                    ops::pg::script(pool, &steps, transactional, timeout).await
                }
                #[cfg(feature = "mariadb")]
                SqlPool::MariaDb(pool) => {
                    ops::mysql::script(pool, &steps, transactional, timeout).await
                }
                #[cfg(feature = "sqlite")]
                SqlPool::Sqlite(pool) => {
                    ops::sqlite::script(pool, &steps, transactional, timeout).await
                }
            };
            pool.close().await;
            result
        })
    }
}

/// create the sqlite file (and its parent directory) when missing. returns whether it was created.
#[cfg(feature = "sqlite")]
async fn ensure_sqlite(connection: &str) -> Result<bool, SendableError> {
    let options = SqliteConnectOptions::from_str(connection).map_err(connect_error)?;
    let path = options.get_filename().to_path_buf();
    let existed = path.exists();

    if let Some(parent) = path.parent()
        && !parent.as_os_str().is_empty()
    {
        std::fs::create_dir_all(parent).map_err(|err| CONNECTION_FAILED.error(err.to_string()))?;
    }

    let connection = options
        .create_if_missing(true)
        .connect()
        .await
        .map_err(connect_error)?;
    let _ = connection.close().await;

    Ok(!existed)
}

/// derive the maintenance connection for postgres by pointing the same credentials at `postgres`.
#[cfg(feature = "postgres")]
async fn ensure_postgres(
    connection: &str,
    admin: Option<&str>,
    database: Option<&str>,
    timeout: Duration,
) -> Result<bool, SendableError> {
    let options = PgConnectOptions::from_str(connection).map_err(connect_error)?;
    let target = database
        .map(str::to_string)
        .or_else(|| options.get_database().map(str::to_string))
        .ok_or_else(|| {
            DATABASE_MISSING.error("could not determine the database name from 'connection'")
        })?;

    let admin_pool = match admin {
        Some(admin) => PgPool::connect(admin).await.map_err(connect_error)?,
        None => {
            let maintenance = PgConnectOptions::from_str(connection)
                .map_err(connect_error)?
                .database("postgres");
            PgPool::connect_with(maintenance)
                .await
                .map_err(|err| DATABASE_MISSING.error(format!(
                    "database '{target}' may not exist and no 'admin_connection' was supplied to create it: {err}"
                )))?
        }
    };

    let exists = ops::pg::query(
        &admin_pool,
        "select 1 from pg_database where datname = $1",
        &[Value::String(target.clone())],
        timeout,
    )
    .await?;

    if exists.row_count() > 0 {
        admin_pool.close().await;
        return Ok(false);
    }

    // `create database` cannot be parameterized, so the name is quoted rather than bound.
    let create = format!("create database \"{}\"", target.replace('"', ""));
    let result = ops::pg::execute(&admin_pool, &create, &[], timeout).await;
    admin_pool.close().await;
    result?;
    Ok(true)
}

#[cfg(feature = "mariadb")]
async fn ensure_mariadb(
    connection: &str,
    admin: Option<&str>,
    database: Option<&str>,
    timeout: Duration,
) -> Result<bool, SendableError> {
    let options = MySqlConnectOptions::from_str(connection).map_err(connect_error)?;
    let target = database
        .map(str::to_string)
        .or_else(|| options.get_database().map(str::to_string))
        .ok_or_else(|| {
            DATABASE_MISSING.error("could not determine the database name from 'connection'")
        })?;

    let admin_pool = match admin {
        Some(admin) => MySqlPool::connect(admin).await.map_err(connect_error)?,
        None => {
            // mysql allows a connection with no default schema, which is enough to create one.
            let maintenance = MySqlConnectOptions::from_str(connection)
                .map_err(connect_error)?
                .database("information_schema");
            MySqlPool::connect_with(maintenance)
                .await
                .map_err(|err| DATABASE_MISSING.error(format!(
                    "database '{target}' may not exist and no 'admin_connection' was supplied to create it: {err}"
                )))?
        }
    };

    let exists = ops::mysql::query(
        &admin_pool,
        "select 1 from information_schema.schemata where schema_name = ?",
        &[Value::String(target.clone())],
        timeout,
    )
    .await?;

    if exists.row_count() > 0 {
        admin_pool.close().await;
        return Ok(false);
    }

    let create = format!("create database `{}`", target.replace('`', ""));
    let result = ops::mysql::execute(&admin_pool, &create, &[], timeout).await;
    admin_pool.close().await;
    result?;
    Ok(true)
}

/// one query per engine returning `(table_schema, table_name, column_name, data_type, nullable)`
/// so schema grouping is shared instead of written three times.
fn inspect_sql(engine: Engine) -> &'static str {
    match engine {
        Engine::Sqlite => {
            "select '' as table_schema, m.name as table_name, p.name as column_name, \
             p.type as data_type, case p.\"notnull\" when 0 then 'YES' else 'NO' end as is_nullable \
             from sqlite_master m join pragma_table_info(m.name) p \
             where m.type = 'table' and m.name not like 'sqlite_%' order by m.name, p.cid"
        }
        Engine::Postgres => {
            "select table_schema, table_name, column_name, data_type, is_nullable \
             from information_schema.columns \
             where table_schema not in ('pg_catalog', 'information_schema') \
             order by table_schema, table_name, ordinal_position"
        }
        // the casts and aliases are load-bearing, not decoration. mysql 8 serves
        // `information_schema` out of the data dictionary: it labels the columns `TABLE_SCHEMA`
        // rather than `table_schema`, and hands the values over as VARBINARY, which decodes to a
        // hex blob instead of text. an explicit alias pins the label and the cast pins the type,
        // so both engines answer in the same shape. mariadb already does this and is unaffected.
        Engine::Mariadb => {
            "select cast(table_schema as char) as table_schema, \
             cast(table_name as char) as table_name, \
             cast(column_name as char) as column_name, \
             cast(data_type as char) as data_type, \
             cast(is_nullable as char) as is_nullable \
             from information_schema.columns where table_schema = database() \
             order by table_name, ordinal_position"
        }
    }
}

/// fold the flat column listing into one entry per table, preserving query order.
fn group_tables(rows: &RowSet) -> Vec<TableInfo> {
    // case-insensitive: engines disagree on how they label `information_schema` columns, and the
    // query above pins the case only for the engines whose text we control.
    let index_of = |name: &str| {
        rows.columns
            .iter()
            .position(|column| column.name.eq_ignore_ascii_case(name))
    };
    let (Some(schema_idx), Some(table_idx), Some(column_idx), Some(type_idx), Some(null_idx)) = (
        index_of("table_schema"),
        index_of("table_name"),
        index_of("column_name"),
        index_of("data_type"),
        index_of("is_nullable"),
    ) else {
        return Vec::new();
    };

    let text = |row: &[Value], idx: usize| match row.get(idx) {
        Some(Value::String(value)) => value.clone(),
        Some(Value::Null) | None => String::new(),
        Some(other) => other.to_string(),
    };

    let mut tables: Vec<TableInfo> = Vec::new();
    for row in &rows.rows {
        let schema = text(row, schema_idx);
        let table = text(row, table_idx);
        let summary = ColumnSummary {
            name: text(row, column_idx),
            native_type: text(row, type_idx),
            nullable: text(row, null_idx).eq_ignore_ascii_case("yes"),
        };

        let schema = (!schema.is_empty()).then_some(schema);
        match tables
            .iter_mut()
            .find(|existing| existing.name == table && existing.schema == schema)
        {
            Some(existing) => existing.columns.push(summary),
            None => tables.push(TableInfo {
                name: table,
                schema,
                columns: vec![summary],
            }),
        }
    }

    tables
}
