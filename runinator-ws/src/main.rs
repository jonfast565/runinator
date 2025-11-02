use std::sync::Arc;

use clap::{Parser, ValueEnum};
use log::info;
use runinator_database::{initialize_database, postgres::PostgresDb, sqlite::SqliteDb};
use runinator_models::errors::SendableError;
use tokio::sync::Notify;

use runinator_ws::run_webserver;

#[derive(Clone, Debug, ValueEnum)]
enum DatabaseKind {
    Sqlite,
    Postgres,
}

#[derive(Debug, Parser)]
#[command(author, version, about, long_about = None)]
struct CliArgs {
    /// Webservice port to bind to, defaults to 8080
    #[arg(long, default_value_t = 8080)]
    port: u16,

    /// Database backend to use
    #[arg(long, value_enum, default_value_t = DatabaseKind::Sqlite)]
    database: DatabaseKind,

    /// Path to the SQLite database file (used when --database=sqlite)
    #[arg(long, default_value = "runinator.db")]
    sqlite_path: String,

    /// Connection string for the database (required when --database=postgres)
    #[arg(long)]
    database_url: Option<String>,
}

#[tokio::main]
async fn main() -> Result<(), SendableError> {
    env_logger::init();
    let args = CliArgs::parse();

    let notify = Arc::new(Notify::new());
    let shutdown_listener = notify.clone();
    tokio::spawn(async move {
        if let Err(err) = tokio::signal::ctrl_c().await {
            log::error!("Failed to listen for shutdown signal: {}", err);
            return;
        }
        info!("Shutdown signal received, stopping web server...");
        shutdown_listener.notify_waiters();
    });

    let CliArgs {
        port,
        database,
        sqlite_path,
        database_url,
    } = args;

    match database {
        DatabaseKind::Sqlite => {
            info!(
                "Starting Runinator webservice with SQLite database at {}",
                sqlite_path
            );
            let db = Arc::new(SqliteDb::new(&sqlite_path).await?);
            initialize_database(&db).await?;
            run_webserver(db, notify.clone(), port).await?;
        }
        DatabaseKind::Postgres => {
            let url = database_url
                .ok_or_else(|| {
                    std::io::Error::new(
                        std::io::ErrorKind::InvalidInput,
                        "--database-url must be provided when --database=postgres",
                    )
                })
                .map_err(|err| -> SendableError { Box::new(err) })?;

            info!("Starting Runinator webservice with Postgres database");
            let db = Arc::new(PostgresDb::new(&url).await?);
            initialize_database(&db).await?;
            run_webserver(db, notify.clone(), port).await?;
        }
    }

    Ok(())
}
