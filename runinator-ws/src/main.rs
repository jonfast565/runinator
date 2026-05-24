mod config;
use std::sync::Arc;

use clap::Parser;
use log::info;
use runinator_broker::{
    Broker,
    adapters::{kafka::KafkaBrokerConfig, rabbitmq::RabbitMqBrokerConfig},
    http::client::HttpBroker,
    in_memory::InMemoryBroker,
    tcp::client::TcpBroker,
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
        broker_action_topic,
        broker_control_topic,
        broker_result_topic,
        broker_client_id,
    } = args;
    let broker = build_broker(
        &broker_backend,
        &broker_endpoint,
        KafkaBrokerConfig::new(broker_endpoint.clone())
            .with_topics(
                broker_action_topic.clone(),
                broker_control_topic.clone(),
                broker_result_topic.clone(),
            )
            .with_client_id(broker_client_id.clone()),
        RabbitMqBrokerConfig::new(broker_endpoint.clone())
            .with_queues(
                broker_action_topic,
                broker_control_topic,
                broker_result_topic,
            )
            .with_client_id(broker_client_id),
    )
    .await?;

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

async fn build_broker(
    backend: &str,
    endpoint: &str,
    kafka_config: KafkaBrokerConfig,
    rabbitmq_config: RabbitMqBrokerConfig,
) -> Result<Arc<dyn Broker>, SendableError> {
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
        "kafka" => build_kafka_broker(kafka_config),
        "rabbitmq" => build_rabbitmq_broker(rabbitmq_config).await,
        other => Err(Box::new(RuntimeError::new(
            "ws.broker.unknown_backend".into(),
            format!("Unknown broker backend '{other}'"),
        ))),
    }
}

#[cfg(feature = "kafka")]
fn build_kafka_broker(config: KafkaBrokerConfig) -> Result<Arc<dyn Broker>, SendableError> {
    let broker = runinator_broker::adapters::kafka::KafkaBroker::new(config).map_err(
        |err| -> SendableError {
            Box::new(RuntimeError::new("ws.broker.kafka".into(), err.to_string()))
        },
    )?;
    Ok(Arc::new(broker))
}

#[cfg(not(feature = "kafka"))]
fn build_kafka_broker(_config: KafkaBrokerConfig) -> Result<Arc<dyn Broker>, SendableError> {
    Err(Box::new(RuntimeError::new(
        "ws.broker.kafka_feature_disabled".into(),
        "Broker backend 'kafka' requires building runinator-ws with --features kafka".into(),
    )))
}

#[cfg(feature = "rabbitmq")]
async fn build_rabbitmq_broker(
    config: RabbitMqBrokerConfig,
) -> Result<Arc<dyn Broker>, SendableError> {
    let broker = runinator_broker::adapters::rabbitmq::RabbitMqBroker::connect(config)
        .await
        .map_err(|err| -> SendableError {
            Box::new(RuntimeError::new(
                "ws.broker.rabbitmq".into(),
                err.to_string(),
            ))
        })?;
    Ok(Arc::new(broker))
}

#[cfg(not(feature = "rabbitmq"))]
async fn build_rabbitmq_broker(
    _config: RabbitMqBrokerConfig,
) -> Result<Arc<dyn Broker>, SendableError> {
    Err(Box::new(RuntimeError::new(
        "ws.broker.rabbitmq_feature_disabled".into(),
        "Broker backend 'rabbitmq' requires building runinator-ws with --features rabbitmq".into(),
    )))
}
