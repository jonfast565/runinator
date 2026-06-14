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
use runinator_database::{mysql::MySqlDb, postgres::PostgresDb, sqlite::SqliteDb};
use runinator_models::errors::SendableError;
use tokio::sync::Notify;
use uuid::Uuid;

use runinator_ws::{AuthOptions, ReplicaAdvertisement, run_webserver};

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
        disable_gossip,
        announce_address,
        announce_base_path,
        gossip_interval_seconds,
        broker_backend,
        broker_endpoint,
        broker_action_topic,
        broker_control_topic,
        broker_result_topic,
        broker_client_id,
        advertise_host,
        auth_enabled,
        auth_access_ttl_seconds,
        auth_refresh_ttl_seconds,
    } = args;
    let auth_options = AuthOptions {
        enabled: auth_enabled,
        access_ttl_secs: auth_access_ttl_seconds,
        refresh_ttl_secs: auth_refresh_ttl_seconds,
    };
    // treat a blank advertise host as unset so the replica list omits it rather than storing "".
    let advertise_host = {
        let trimmed = advertise_host.trim();
        (!trimmed.is_empty()).then(|| trimmed.to_string())
    };
    // advertise the backends this replica runs on so the replica list has parity with worker/waker.
    let database_backend = match &database {
        DatabaseKind::Sqlite => "sqlite",
        DatabaseKind::Postgres => "postgres",
        DatabaseKind::Mysql => "mysql",
    };
    let advertisement = ReplicaAdvertisement {
        host: advertise_host,
        attributes: runinator_models::json!({
            "broker_backend": broker_backend.clone(),
            "broker_client_id": broker_client_id.clone(),
            "database_backend": database_backend,
        }),
    };
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
    if !should_spawn_gossip_advertiser(disable_gossip) {
        info!("Web service gossip advertisements disabled");
    } else {
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
    }

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
            run_webserver(
                db,
                notify.clone(),
                port,
                broker,
                advertisement.clone(),
                auth_options.clone(),
            )
            .await?;
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
            run_webserver(
                db,
                notify.clone(),
                port,
                broker,
                advertisement.clone(),
                auth_options.clone(),
            )
            .await?;
        }
        DatabaseKind::Mysql => {
            let url = database_url
                .ok_or_else(|| {
                        std::io::Error::new(
                            std::io::ErrorKind::InvalidInput,
                            "--database-url must be provided when --database=mysql or --database=mariadb",
                        )
                    })
                .map_err(|err| -> SendableError { Box::new(err) })?;

            info!("Starting Runinator webservice with MySQL/MariaDB database");
            let db = Arc::new(MySqlDb::new(&url).await?);
            run_webserver(
                db,
                notify.clone(),
                port,
                broker,
                advertisement.clone(),
                auth_options.clone(),
            )
            .await?;
        }
    }

    Ok(())
}

fn should_spawn_gossip_advertiser(disable_gossip: bool) -> bool {
    !disable_gossip
}

async fn build_broker(
    backend: &str,
    endpoint: &str,
    kafka_config: KafkaBrokerConfig,
    rabbitmq_config: RabbitMqBrokerConfig,
) -> Result<Arc<dyn Broker>, SendableError> {
    let result_channel = match backend {
        "kafka" => kafka_config.result_topic.as_str(),
        "rabbitmq" => rabbitmq_config.result_queue.as_str(),
        _ => "",
    };
    runinator_broker::ensure_named_workflow_result_channel(backend, result_channel)
        .map_err(|err| runinator_ws::errors::BROKER_WORKFLOW_RESULTS.error(err))?;

    let broker: Arc<dyn Broker> = match backend {
        "http" => {
            let url = reqwest::Url::parse(endpoint)
                .map_err(|err| runinator_ws::errors::BROKER_INVALID_ENDPOINT.error(err))?;
            let client = reqwest::Client::builder()
                .build()
                .map_err(|err| runinator_ws::errors::BROKER_CLIENT.error(err))?;
            Arc::new(HttpBroker::new(url, client))
        }
        "in-memory" => Arc::new(InMemoryBroker::new()),
        "tcp" => Arc::new(TcpBroker::new(endpoint.to_string())),
        "kafka" => build_kafka_broker(kafka_config)?,
        "rabbitmq" => build_rabbitmq_broker(rabbitmq_config).await?,
        other => {
            return Err(runinator_ws::errors::BROKER_UNKNOWN_BACKEND.error(format!("'{other}'")));
        }
    };

    runinator_broker::ensure_workflow_result_channels_supported(backend, broker.as_ref())
        .map_err(|err| runinator_ws::errors::BROKER_WORKFLOW_RESULTS.error(err))?;

    Ok(broker)
}

#[cfg(feature = "kafka")]
fn build_kafka_broker(config: KafkaBrokerConfig) -> Result<Arc<dyn Broker>, SendableError> {
    let broker = runinator_broker::adapters::kafka::KafkaBroker::new(config)
        .map_err(|err| runinator_ws::errors::BROKER_KAFKA.error(err))?;
    Ok(Arc::new(broker))
}

#[cfg(not(feature = "kafka"))]
fn build_kafka_broker(_config: KafkaBrokerConfig) -> Result<Arc<dyn Broker>, SendableError> {
    Err(runinator_ws::errors::BROKER_KAFKA_FEATURE_DISABLED
        .error("build runinator-ws with --features kafka"))
}

#[cfg(feature = "rabbitmq")]
async fn build_rabbitmq_broker(
    config: RabbitMqBrokerConfig,
) -> Result<Arc<dyn Broker>, SendableError> {
    let broker = runinator_broker::adapters::rabbitmq::RabbitMqBroker::connect(config)
        .await
        .map_err(|err| runinator_ws::errors::BROKER_RABBITMQ.error(err))?;
    Ok(Arc::new(broker))
}

#[cfg(not(feature = "rabbitmq"))]
async fn build_rabbitmq_broker(
    _config: RabbitMqBrokerConfig,
) -> Result<Arc<dyn Broker>, SendableError> {
    Err(runinator_ws::errors::BROKER_RABBITMQ_FEATURE_DISABLED
        .error("build runinator-ws with --features rabbitmq"))
}

#[cfg(test)]
mod startup_tests {
    use super::*;

    #[test]
    fn disable_gossip_skips_advertiser_startup() {
        assert!(!should_spawn_gossip_advertiser(true));
        assert!(should_spawn_gossip_advertiser(false));
    }

    #[tokio::test]
    async fn build_broker_rejects_kafka_without_result_topic() {
        let err = match build_broker(
            "kafka",
            "localhost:9092",
            KafkaBrokerConfig::new("localhost:9092").with_topics("actions", "control", " "),
            RabbitMqBrokerConfig::new("amqp://127.0.0.1:5672/%2f"),
        )
        .await
        {
            Ok(_) => panic!("expected kafka result channel startup guard to fail"),
            Err(err) => err,
        };

        assert!(err.to_string().contains("Broker backend 'kafka'"));
        assert!(err.to_string().contains("non-empty workflow result topic"));
    }

    #[tokio::test]
    async fn build_broker_rejects_rabbitmq_without_result_queue() {
        let err = match build_broker(
            "rabbitmq",
            "amqp://127.0.0.1:5672/%2f",
            KafkaBrokerConfig::new("localhost:9092"),
            RabbitMqBrokerConfig::new("amqp://127.0.0.1:5672/%2f")
                .with_queues("actions", "control", ""),
        )
        .await
        {
            Ok(_) => panic!("expected rabbitmq result channel startup guard to fail"),
            Err(err) => err,
        };

        assert!(err.to_string().contains("Broker backend 'rabbitmq'"));
        assert!(err.to_string().contains("non-empty workflow result queue"));
    }
}
