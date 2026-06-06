use std::{fs, path::PathBuf, str::FromStr};

use futures_util::stream::StreamExt;
use log::{debug, info};
use runinator_models::errors::SendableError;
use sqlx::{
    ConnectOptions, Executor, MySqlPool,
    migrate::Migrator,
    mysql::{MySqlConnectOptions, MySqlPoolOptions},
};

use crate::{backend::SqlBackend, queries::SqlDialect};

static MYSQL_MIGRATOR: Migrator = sqlx::migrate!("./migrations/mysql");

pub struct MySqlDb {
    pub pool: MySqlPool,
}

#[cfg(test)]
#[path = "mysql_tests.rs"]
mod tests;

impl MySqlDb {
    pub async fn new(connection_str: &str) -> Result<Self, SendableError> {
        let options = MySqlConnectOptions::from_str(connection_str)?
            .log_statements(log::LevelFilter::Debug)
            .log_slow_statements(log::LevelFilter::Warn, std::time::Duration::from_secs(1));

        let pool = MySqlPoolOptions::new().connect_with(options).await?;
        Ok(Self { pool })
    }

    pub async fn run_migrations(&self) -> Result<(), SendableError> {
        info!("Running embedded MySQL/MariaDB migrations");
        MYSQL_MIGRATOR
            .run(&self.pool)
            .await
            .map_err(|err| -> SendableError { Box::new(err) })?;
        Ok(())
    }

    async fn execute_script(&self, script: &str) -> Result<(), SendableError> {
        let sql = script.trim();
        if sql.is_empty() {
            return Ok(());
        }

        for statement in sql.split(';') {
            let stmt = statement.trim();
            if stmt.is_empty() {
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

impl SqlBackend for MySqlDb {
    type Db = sqlx::MySql;

    fn pool(&self) -> &MySqlPool {
        &self.pool
    }

    fn dialect(&self) -> SqlDialect {
        SqlDialect::MySql
    }

    async fn init(&self, paths: &[String]) -> Result<(), SendableError> {
        self.run_migrations().await?;
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
