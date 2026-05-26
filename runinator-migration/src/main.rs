use std::process::ExitCode;

use clap::{Parser, ValueEnum};
use log::{error, info};
use runinator_database::{postgres::PostgresDb, sqlite::SqliteDb};
use runinator_models::errors::SendableError;

#[derive(ValueEnum, Debug, Clone, Copy)]
enum Backend {
    Sqlite,
    Postgres,
}

#[derive(Parser, Debug)]
#[command(
    name = "runinator-migration",
    about = "Apply Runinator database migrations and exit."
)]
struct Cli {
    /// Backend to migrate.
    #[arg(long, value_enum)]
    database: Backend,

    /// Connection string. For sqlite, a filesystem path (file:/path/to/runinator.db
    /// or just /path/to/runinator.db). For postgres, a postgres:// URL.
    /// Falls back to $DATABASE_URL when not set.
    #[arg(long)]
    database_url: Option<String>,
}

#[tokio::main]
async fn main() -> ExitCode {
    if std::env::var_os("RUST_LOG").is_none() {
        unsafe {
            std::env::set_var("RUST_LOG", "info");
        }
    }
    env_logger::init();

    match run().await {
        Ok(()) => {
            info!("Migrations applied successfully.");
            ExitCode::SUCCESS
        }
        Err(err) => {
            error!("Migration failed: {err}");
            ExitCode::FAILURE
        }
    }
}

async fn run() -> Result<(), SendableError> {
    let cli = Cli::parse();
    let url = cli
        .database_url
        .or_else(|| std::env::var("DATABASE_URL").ok())
        .ok_or_else(|| -> SendableError {
            "missing connection string: pass --database-url or set DATABASE_URL".into()
        })?;

    match cli.database {
        Backend::Sqlite => {
            info!("Connecting to sqlite at {url}");
            let db = SqliteDb::new(&url).await?;
            db.run_migrations().await?;
        }
        Backend::Postgres => {
            info!("Connecting to postgres");
            let db = PostgresDb::new(&url).await?;
            db.run_migrations().await?;
        }
    }
    Ok(())
}
