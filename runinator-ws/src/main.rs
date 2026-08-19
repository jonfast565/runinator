mod config;
mod service;
use std::sync::Arc;

use clap::Parser;
use log::info;
#[cfg(feature = "http")]
use runinator_broker::http::client::HttpBroker;
#[cfg(feature = "tcp")]
use runinator_broker::tcp::client::TcpBroker;
use runinator_broker::{
    Broker,
    adapters::{kafka::KafkaBrokerConfig, rabbitmq::RabbitMqBrokerConfig},
    in_memory::InMemoryBroker,
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
#[cfg(feature = "sqlite")]
use runinator_utilities::app_data;
use runinator_utilities::startup;
use service::WebService;

#[tokio::main]
async fn main() -> Result<(), SendableError> {
    WebService::new().run().await
}

async fn run_process() -> Result<(), SendableError> {
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
        announce_scheme,
        announce_relay_path,
        cluster_id,
        announce_spki_pin,
        gossip_interval_seconds,
        broker_backend,
        broker_endpoint,
        broker_action_topic,
        broker_control_topic,
        broker_agent_topic,
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
    // A single-backend build compiles the other dispatch arms out. Keep their CLI fields accepted
    // (so the command surface stays stable) without warning when that happens.
    let _ = (&sqlite_path, &database_url);
    if !matches!(announce_scheme.as_str(), "http" | "https") {
        return Err(
            format!("--announce-scheme must be http or https, got '{announce_scheme}'").into(),
        );
    }
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
            .with_agent_topic(broker_agent_topic.clone())
            .with_client_id(broker_client_id.clone()),
        RabbitMqBrokerConfig::new(broker_endpoint.clone())
            .with_queues(
                broker_action_topic,
                broker_control_topic,
                broker_result_topic,
            )
            .with_agent_queue_prefix(broker_agent_topic)
            .with_client_id(broker_client_id),
    )
    .await?;

    let service_id = Uuid::new_v4();
    let advertised_service_url = format!(
        "{}://{}:{}{}",
        announce_scheme,
        announce_address,
        port,
        if announce_base_path.starts_with('/') {
            announce_base_path.clone()
        } else {
            format!("/{announce_base_path}")
        }
    );
    let cluster_id = cluster_id.unwrap_or_else(|| {
        runinator_comm::discovery::web::cluster_id_for_service_url(&advertised_service_url)
    });
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
            announce_scheme: announce_scheme.clone(),
            announce_relay_path: announce_relay_path.clone(),
            cluster_id,
            enrollment_enabled: true,
            spki_pin: announce_spki_pin.clone(),
            version: Some(env!("CARGO_PKG_VERSION").to_string()),
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
    let _ = endpoint;
    let result_channel = match backend {
        "kafka" => kafka_config.result_topic.as_str(),
        "rabbitmq" => rabbitmq_config.result_queue.as_str(),
        _ => "",
    };
    runinator_broker::ensure_named_workflow_result_channel(backend, result_channel)
        .map_err(|err| runinator_ws::errors::BROKER_WORKFLOW_RESULTS.error(err))?;

    let broker: Arc<dyn Broker> = match backend {
        #[cfg(feature = "http")]
        "http" => {
            let url = reqwest::Url::parse(endpoint)
                .map_err(|err| runinator_ws::errors::BROKER_INVALID_ENDPOINT.error(err))?;
            let client = reqwest::Client::builder()
                .build()
                .map_err(|err| runinator_ws::errors::BROKER_CLIENT.error(err))?;
            Arc::new(HttpBroker::new(url, client))
        }
        "in-memory" => Arc::new(InMemoryBroker::new()),
        #[cfg(feature = "tcp")]
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
#[path = "startup_tests.rs"]
mod startup_tests;
