mod config;
mod discovery;

use std::{sync::Arc};

use clap::Parser;
use log::info;
use runinator_database::{postgres::PostgresDb, sqlite::SqliteDb};
use runinator_models::errors::SendableError;
use tokio::{sync::Notify};
use uuid::Uuid;

use runinator_ws::run_webserver;

use crate::config::{CliArgs, DatabaseKind};
use runinator_utilities::{startup};

#[tokio::main]
async fn main() -> Result<(), SendableError> {
    startup::startup("Runinator Web Service")?;
    
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
        gossip_bind,
        gossip_port,
        gossip_targets,
        announce_address,
        announce_base_path,
        gossip_interval_seconds,
    } = args;

    let service_id = Uuid::new_v4();
    discovery::spawn_gossip_advertiser_ws(
        service_id,
        gossip_bind,
        gossip_port,
        gossip_targets,
        announce_address.clone(),
        announce_base_path.clone(),
        gossip_interval_seconds,
        notify.clone(),
        port,
    );

    match database {
        DatabaseKind::Sqlite => {
            info!(
                "Starting Runinator webservice with SQLite database at {}",
                sqlite_path
            );
            let db = Arc::new(SqliteDb::new(&sqlite_path).await?);
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
            run_webserver(db, notify.clone(), port).await?;
        }
    }

    Ok(())
}

