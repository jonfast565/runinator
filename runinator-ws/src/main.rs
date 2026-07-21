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
use runinator_db_cli::{DatabaseBackend, dispatch_database};
use runinator_models::errors::SendableError;
use tokio::sync::Notify;
use uuid::Uuid;

use runinator_ws::{
    AuthOptions, OverloadConfig, RateLimitConfig, ReplicaAdvertisement, run_webserver,
};

use crate::config::CliArgs;
use runinator_comm::discovery::{WebServiceAdvertiserConfig, spawn_web_service_advertiser};
use runinator_utilities::{app_data, startup};

#[tokio::main]
async fn main() -> Result<(), SendableError> {
    // this binary links rustls with both ring (jsonwebtoken) and aws-lc-rs (aws sdk) crypto backends,
    // leaving no unambiguous process-default CryptoProvider. install one before any rustls default-path
    // config is built (e.g. the kubernetes node provisioner's kube client), otherwise that path panics.
    // an Err means a provider is already installed, which is fine.
    let _ = rustls::crypto::ring::default_provider().install_default();

    // held for the process lifetime so otel signals flush on shutdown.
    let _telemetry = startup::startup("Runinator Web Service")?;

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
        instance_id,
        auth_enabled,
        auth_access_ttl_seconds,
        auth_refresh_ttl_seconds,
        rate_limit_enabled,
        rate_limit_rps,
        rate_limit_burst,
        overload_protection_enabled,
        max_concurrent_requests,
        request_timeout_seconds,
        run_engine,
    } = args;
    let auth_options = AuthOptions {
        enabled: auth_enabled,
        access_ttl_secs: auth_access_ttl_seconds,
        refresh_ttl_secs: auth_refresh_ttl_seconds,
    };
    let rate_limit_options = RateLimitConfig {
        enabled: rate_limit_enabled,
        requests_per_second: rate_limit_rps,
        burst: rate_limit_burst,
    };
    let overload_options = OverloadConfig {
        enabled: overload_protection_enabled,
        max_concurrent_requests,
        request_timeout: std::time::Duration::from_secs(request_timeout_seconds),
    };
    // treat a blank advertise host as unset so the replica list omits it rather than storing "".
    let advertise_host = {
        let trimmed = advertise_host.trim();
        (!trimmed.is_empty()).then(|| trimmed.to_string())
    };
    // advertise the backends this replica runs on so the replica list has parity with worker/waker.
    let database_backend = match &database {
        DatabaseBackend::Sqlite => "sqlite",
        DatabaseBackend::Postgres => "postgres",
        DatabaseBackend::Mysql => "mysql",
    };
    let advertisement = ReplicaAdvertisement {
        host: advertise_host,
        instance_id: instance_id.and_then(|value| {
            let trimmed = value.trim();
            (!trimmed.is_empty()).then(|| trimmed.to_string())
        }),
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

    info!("Starting Runinator webservice with {database_backend} database");
    dispatch_database!(
        database,
        sqlite: {
            let sqlite_path = sqlite_path.unwrap_or(app_data::default_sqlite_path()?);
            if let Some(parent) = sqlite_path.parent() {
                tokio::fs::create_dir_all(parent).await?;
            }
            info!("SQLite database file at {}", sqlite_path.display());
            sqlite_path.to_string_lossy().into_owned()
        },
        url: database_url
            .clone()
            .ok_or_else(|| -> SendableError {
                "--database-url must be provided when --database=postgres/mysql/mariadb".into()
            })?,
        |db| {
            run_webserver(
                db,
                notify.clone(),
                port,
                broker,
                advertisement.clone(),
                auth_options.clone(),
                rate_limit_options,
                overload_options,
                run_engine,
            )
            .await?;
        }
    );

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
        "kafka" => runinator_broker::build_kafka_broker(kafka_config)
            .map_err(|err| runinator_ws::errors::BROKER_KAFKA.error(err))?,
        "rabbitmq" => runinator_broker::build_rabbitmq_broker(rabbitmq_config)
            .await
            .map_err(|err| runinator_ws::errors::BROKER_RABBITMQ.error(err))?,
        other => {
            return Err(runinator_ws::errors::BROKER_UNKNOWN_BACKEND.error(format!("'{other}'")));
        }
    };

    runinator_broker::ensure_workflow_result_channels_supported(backend, broker.as_ref())
        .map_err(|err| runinator_ws::errors::BROKER_WORKFLOW_RESULTS.error(err))?;

    // wrap the concrete backend so every broker operation emits otel metrics tagged with the backend.
    Ok(runinator_broker::instrument(broker, backend.to_string()))
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
