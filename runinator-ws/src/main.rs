mod config;
use std::sync::Arc;

use clap::Parser;
use log::info;
use runinator_broker::{
    Broker, http::client::HttpBroker, in_memory::InMemoryBroker, tcp::client::TcpBroker,
};
use runinator_database::{postgres::PostgresDb, sqlite::SqliteDb};
use runinator_models::errors::{RuntimeError, SendableError};
use tokio::sync::Notify;
use uuid::Uuid;

use runinator_ws::run_webserver;

use crate::config::{CliArgs, DatabaseKind};
use runinator_comm::discovery::{WebServiceAdvertiserConfig, spawn_web_service_advertiser};
use runinator_utilities::{app_data, startup};

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
        broker_backend,
        broker_endpoint,
    } = args;
    let broker = build_broker(&broker_backend, &broker_endpoint)?;

    let service_id = Uuid::new_v4();
    spawn_web_service_advertiser(WebServiceAdvertiserConfig {
        service_id,
        bind_addr: gossip_bind,
        gossip_port,
        extra_targets: gossip_targets,
        announce_address: announce_address.clone(),
        announce_base_path: announce_base_path.clone(),
        interval_seconds: gossip_interval_seconds,
        shutdown: notify.clone(),
        service_port: port,
    });

    match database {
        DatabaseKind::Sqlite => {
            let sqlite_path = sqlite_path.unwrap_or(app_data::default_sqlite_path()?);
            if let Some(parent) = sqlite_path.parent() {
                tokio::fs::create_dir_all(parent).await?;
            }
            info!(
                "Starting Runinator webservice with SQLite database at {}",
                sqlite_path.display()
            );
            let sqlite_path = sqlite_path.to_string_lossy();
            let db = Arc::new(SqliteDb::new(sqlite_path.as_ref()).await?);
            run_webserver(db, notify.clone(), port, broker).await?;
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
            run_webserver(db, notify.clone(), port, broker).await?;
        }
    }

    Ok(())
}

fn build_broker(backend: &str, endpoint: &str) -> Result<Arc<dyn Broker>, SendableError> {
    match backend {
        "http" => {
            let url = reqwest::Url::parse(endpoint).map_err(|err| -> SendableError {
                Box::new(RuntimeError::new(
                    "ws.broker.invalid_endpoint".into(),
                    err.to_string(),
                ))
            })?;
            let client = reqwest::Client::builder()
                .build()
                .map_err(|err| -> SendableError {
                    Box::new(RuntimeError::new(
                        "ws.broker.client".into(),
                        err.to_string(),
                    ))
                })?;
            Ok(Arc::new(HttpBroker::new(url, client)))
        }
        "in-memory" => Ok(Arc::new(InMemoryBroker::new())),
        "tcp" => Ok(Arc::new(TcpBroker::new(endpoint.to_string()))),
        "rabbitmq" | "kafka" => Err(Box::new(RuntimeError::new(
            "ws.broker.backend_not_ready".into(),
            format!("Broker backend '{backend}' is not implemented yet"),
        ))),
        other => Err(Box::new(RuntimeError::new(
            "ws.broker.unknown_backend".into(),
            format!("Unknown broker backend '{other}'"),
        ))),
    }
}
