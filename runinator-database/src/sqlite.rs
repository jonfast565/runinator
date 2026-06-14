use std::{fs, path::PathBuf};

use futures_util::stream::StreamExt;
use log::{debug, info};
use runinator_models::errors::SendableError;
use sqlx::{ConnectOptions, Executor, SqlitePool, migrate::Migrator, sqlite::SqliteConnectOptions};

use crate::{backend::SqlBackend, queries::SqlDialect};

static SQLITE_MIGRATOR: Migrator = sqlx::migrate!("./migrations/sqlite");

pub struct SqliteDb {
    pub pool: SqlitePool,
}

#[cfg(test)]
#[path = "sqlite_tests.rs"]
mod tests;

impl SqliteDb {
    pub async fn new(filename: &str) -> Result<Self, SendableError> {
        let options = SqliteConnectOptions::new()
            .filename(filename)
            .create_if_missing(true);
        let options_with_logs = options
            .log_statements(log::LevelFilter::Debug)
            .log_slow_statements(log::LevelFilter::Warn, std::time::Duration::from_secs(1));
        let unmutable_options = options_with_logs.clone();
        let connection = SqlitePool::connect_with(unmutable_options).await?;
        Ok(SqliteDb { pool: connection })
    }

    pub async fn bootstrap(&self) -> Result<(), SendableError> {
        info!("Running embedded SQLite bootstrap");
        SQLITE_MIGRATOR
            .run(&self.pool)
            .await
            .map_err(|err| -> SendableError { Box::new(err) })?;
        Ok(())
    }

    async fn execute_script(&self, script: &str) -> Result<(), SendableError> {
        let sql = script.trim();
        if sql.is_empty() {
            debug!("No SQL to execute");
            return Ok(());
        }

        for statement in sql.split(';') {
            let stmt = statement.trim();
            if stmt.is_empty() {
                debug!("Skipping empty statement");
                continue;
            }

            let mut stream = self.pool.execute_many(sqlx::query(stmt));
            while let Some(result) = stream.next().await {
                let query_result = result?;
                debug!(
                    "Init scripts: {} row(s) affected",
                    query_result.rows_affected()
                );
            }
        }

        Ok(())
    }
}

impl SqlBackend for SqliteDb {
    type Db = sqlx::Sqlite;

    fn pool(&self) -> &SqlitePool {
        &self.pool
    }

    fn dialect(&self) -> SqlDialect {
        SqlDialect::Sqlite
    }

    async fn init(&self, paths: &[String]) -> Result<(), SendableError> {
        self.bootstrap().await?;
        for path in paths.iter() {
            let path_info = PathBuf::from(path);
            if path_info.extension().and_then(|ext| ext.to_str()) == Some("sql") {
                info!("Running {}", path_info.display());
                let script = fs::read_to_string(path_info.as_path())?;
                self.execute_script(&script).await?;
            }
        }
        Ok(())
    }
}
