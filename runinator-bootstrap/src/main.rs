use std::process::ExitCode;

use clap::Parser;
use runinator_database::{BootstrapOptions, bootstrap_database};
use runinator_db_cli::{DatabaseBackend, dispatch_database};
use runinator_models::errors::SendableError;
use tracing::{error, info};

#[derive(Parser, Debug)]
#[command(
    name = "runinator-bootstrap",
    about = "Apply Runinator database bootstrap and exit."
)]
struct Cli {
    /// Backend to bootstrap. Also reads RUNINATOR_DATABASE.
    #[arg(long, env = "RUNINATOR_DATABASE", value_enum)]
    database: DatabaseBackend,

    /// Connection string. For sqlite, a filesystem path (file:/path/to/runinator.db
    /// or just /path/to/runinator.db). For postgres, a postgres:// URL. For
    /// mysql/mariadb, a mysql:// URL. Also reads RUNINATOR_DATABASE_URL and
    /// falls back to DATABASE_URL when not set.
    #[arg(long, env = "RUNINATOR_DATABASE_URL")]
    database_url: Option<String>,

    /// HS256 signing secret to persist for web-service replicas. When unset, bootstrap generates one.
    #[arg(long, env = "RUNINATOR_AUTH_JWT_SECRET")]
    auth_jwt_secret: Option<String>,

    /// previous HS256 signing secret accepted on verify during a rotation overlap window. set it to
    /// the pre-rotation secret while rotating; leave unset (or empty) to retire the old key.
    #[arg(long, env = "RUNINATOR_AUTH_JWT_SECRET_PREVIOUS")]
    auth_jwt_secret_previous: Option<String>,

    /// `username:password` seeded as an admin when the database has no users yet.
    #[arg(long, env = "RUNINATOR_AUTH_BOOTSTRAP_ADMIN")]
    auth_bootstrap_admin: Option<String>,

    /// reconcile (reset) the bootstrap admin password even when users already exist.
    #[arg(
        long,
        env = "RUNINATOR_AUTH_BOOTSTRAP_ADMIN_FORCE",
        default_value_t = false
    )]
    auth_bootstrap_admin_force: bool,

    /// raw service api key (`<prefix>.<secret>`) seeded for non-interactive local/dev clients.
    #[arg(long, env = "RUNINATOR_AUTH_BOOTSTRAP_SERVICE_API_KEY")]
    auth_bootstrap_service_api_key: Option<String>,

    /// display name attached to the seeded bootstrap service api key.
    #[arg(
        long,
        env = "RUNINATOR_AUTH_BOOTSTRAP_SERVICE_API_KEY_NAME",
        default_value = "bootstrap-service"
    )]
    auth_bootstrap_service_api_key_name: String,
}

#[tokio::main]
async fn main() -> ExitCode {
    // shares the same RUNINATOR_LOG-driven tracing pipeline as ws/worker/waker/archiver. the guard is
    // dropped immediately after startup since this is a one-shot job with no otel signals to flush.
    if let Err(err) = runinator_utilities::startup::startup("Runinator Bootstrap") {
        eprintln!("Bootstrap startup failed: {err}");
        return ExitCode::FAILURE;
    }

    match run().await {
        Ok(()) => {
            info!("bootstrap completed successfully");
            ExitCode::SUCCESS
        }
        Err(err) => {
            error!(
                error_code = runinator_models::errors::error_code_or_unknown(err.as_ref()),
                "bootstrap failed: {err}"
            );
            ExitCode::FAILURE
        }
    }
}

async fn run() -> Result<(), SendableError> {
    let cli = Cli::parse();
    let bootstrap_options = BootstrapOptions {
        auth_jwt_secret: cli.auth_jwt_secret.clone(),
        auth_jwt_secret_previous: cli.auth_jwt_secret_previous.clone(),
        auth_bootstrap_admin: cli.auth_bootstrap_admin.clone(),
        auth_bootstrap_admin_force: cli.auth_bootstrap_admin_force,
        auth_bootstrap_service_api_key: cli.auth_bootstrap_service_api_key.clone(),
        auth_bootstrap_service_api_key_name: Some(cli.auth_bootstrap_service_api_key_name.clone()),
    };
    let url = cli
        .database_url
        .or_else(|| std::env::var("DATABASE_URL").ok())
        .ok_or_else(|| -> SendableError {
            "missing connection string: pass --database-url or set DATABASE_URL".into()
        })?;

    dispatch_database!(
        cli.database,
        sqlite: url.clone(),
        url: url.clone(),
        |db| {
            bootstrap_database(&db, &bootstrap_options).await?;
        }
    );
    Ok(())
}
