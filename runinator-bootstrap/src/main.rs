use std::process::ExitCode;

use clap::{Parser, ValueEnum};
use log::{error, info};
use runinator_database::{
    BootstrapOptions, bootstrap_database, mysql::MySqlDb, postgres::PostgresDb, sqlite::SqliteDb,
};
use runinator_models::errors::SendableError;

#[derive(ValueEnum, Debug, Clone, Copy)]
enum Backend {
    Sqlite,
    Postgres,
    #[value(alias = "mariadb")]
    Mysql,
}

#[derive(Parser, Debug)]
#[command(
    name = "runinator-bootstrap",
    about = "Apply Runinator database bootstrap and exit."
)]
struct Cli {
    /// Backend to bootstrap. Also reads RUNINATOR_DATABASE.
    #[arg(long, env = "RUNINATOR_DATABASE", value_enum)]
    database: Backend,

    /// Connection string. For sqlite, a filesystem path (file:/path/to/runinator.db
    /// or just /path/to/runinator.db). For postgres, a postgres:// URL. For
    /// mysql/mariadb, a mysql:// URL. Also reads RUNINATOR_DATABASE_URL and
    /// falls back to DATABASE_URL when not set.
    #[arg(long, env = "RUNINATOR_DATABASE_URL")]
    database_url: Option<String>,

    /// HS256 signing secret to persist for web-service replicas. When unset, bootstrap generates one.
    #[arg(long, env = "RUNINATOR_AUTH_JWT_SECRET")]
    auth_jwt_secret: Option<String>,

    /// `username:password` seeded as an admin when the database has no users yet.
    #[arg(long, env = "RUNINATOR_AUTH_BOOTSTRAP_ADMIN")]
    auth_bootstrap_admin: Option<String>,
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
            info!("Bootstrap completed successfully.");
            ExitCode::SUCCESS
        }
        Err(err) => {
            error!("Bootstrap failed: {err}");
            ExitCode::FAILURE
        }
    }
}

async fn run() -> Result<(), SendableError> {
    let cli = Cli::parse();
    let bootstrap_options = BootstrapOptions {
        auth_jwt_secret: cli.auth_jwt_secret.clone(),
        auth_bootstrap_admin: cli.auth_bootstrap_admin.clone(),
    };
    let url = cli
        .database_url
        .or_else(|| std::env::var("DATABASE_URL").ok())
        .ok_or_else(|| -> SendableError {
            "missing connection string: pass --database-url or set DATABASE_URL".into()
        })?;

    match cli.database {
        Backend::Sqlite => {
            info!("Connecting to sqlite at {url}");
            let db = std::sync::Arc::new(SqliteDb::new(&url).await?);
            bootstrap_database(&db, &bootstrap_options).await?;
        }
        Backend::Postgres => {
            info!("Connecting to postgres");
            let db = std::sync::Arc::new(PostgresDb::new(&url).await?);
            bootstrap_database(&db, &bootstrap_options).await?;
        }
        Backend::Mysql => {
            info!("Connecting to mysql/mariadb");
            let db = std::sync::Arc::new(MySqlDb::new(&url).await?);
            bootstrap_database(&db, &bootstrap_options).await?;
        }
    }
    Ok(())
}
