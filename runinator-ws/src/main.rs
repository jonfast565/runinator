mod config;
mod service;
use clap::Parser;
use log::info;
use runinator_broker::{BrokerConnectionMode, select_broker_connection};
use runinator_models::errors::SendableError;
use runinator_service_bootstrap::{
    BlobRequest, BrokerClientConfig, BrokerConsumerProfile, DatabaseRequest, ServerBootstrapError,
    ServerResources, dispatch_server_database,
};
use uuid::Uuid;

use runinator_ws::{
    AuthOptions, CorsConfig, OverloadConfig, RateLimitConfig, ReplicaAdvertisement, run_webserver,
};

use crate::config::CliArgs;
use runinator_comm::discovery::{WebServiceAdvertiserConfig, spawn_web_service_advertiser};
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

    let args = CliArgs::parse();

    let CliArgs {
        tui,
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
        broker_mode,
        service_url,
        api_key,
        broker_relay_path,
        broker_effect_topic,
        broker_infrastructure_effect_topic,
        broker_control_topic,
        broker_agent_topic,
        broker_effect_result_topic,
        broker_wake_topic,
        broker_ingress_topic,
        broker_client_id,
        advertise_host,
        instance_id,
        auth_enabled,
        cors_allowed_origins,
        auth_access_ttl_seconds,
        auth_refresh_ttl_seconds,
        rate_limit_enabled,
        rate_limit_rps,
        rate_limit_burst,
        overload_protection_enabled,
        max_concurrent_requests,
        request_timeout_seconds,
        run_engine,
        max_concurrent_ingress,
    } = args;
    let tui = runinator_observability::tui::prepare(tui);
    // A single-backend build compiles the other dispatch arms out. Keep their CLI fields accepted
    // (so the command surface stays stable) without warning when that happens.
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
    let cors_options =
        CorsConfig::new(cors_allowed_origins).map_err(|err| -> SendableError { err.into() })?;
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
    let broker_mode = BrokerConnectionMode::parse(&broker_mode)
        .ok_or_else(|| format!("unknown --broker-mode '{broker_mode}'"))?;
    let broker_client_id_display = broker_client_id.clone();
    let connection = select_broker_connection(
        broker_mode,
        BrokerClientConfig {
            backend: broker_backend,
            endpoint: broker_endpoint,
            effect_topic: broker_effect_topic,
            infrastructure_effect_topic: broker_infrastructure_effect_topic,
            control_topic: broker_control_topic,
            agent_topic: Some(broker_agent_topic),
            effect_result_topic: broker_effect_result_topic,
            client_id: broker_client_id,
            relay_credential: api_key,
            // the engine arms timer wakes and consumes ingress, so it opts into both
            // orchestration channels rather than leaving them at the backend defaults.
            wake_topic: Some(broker_wake_topic),
            ingress_topic: Some(broker_ingress_topic),
        },
        service_url.unwrap_or_default(),
        Some(&broker_relay_path),
    );
    let broker_connection = connection.description()?;
    let broker_config = connection.client_config()?;
    let broker_backend_display = broker_config.backend.clone();

    // advertise the backends this replica runs on so the replica list has parity with worker/waker.
    let database_backend = database.label();
    let advertisement = ReplicaAdvertisement {
        host: advertise_host,
        instance_id: instance_id.and_then(|value| {
            let trimmed = value.trim();
            (!trimmed.is_empty()).then(|| trimmed.to_string())
        }),
        attributes: runinator_models::json!({
            "broker_backend": broker_backend_display.clone(),
            "broker_connection": broker_connection.clone(),
            "broker_client_id": broker_client_id_display,
            "database_backend": database_backend,
        }),
    };
    let resources = ServerResources::builder("Runinator Web Service")
        .broker(broker_config, BrokerConsumerProfile::WorkflowRuntime)
        .blobs(BlobRequest {
            ensure_buckets: true,
        })
        .database(DatabaseRequest {
            backend: database,
            sqlite_path,
            database_url,
        })
        .build()
        .await
        .map_err(map_bootstrap_error)?;
    let shutdown = resources.process().shutdown().clone();
    if tui {
        let dashboard = runinator_observability::tui::install();
        runinator_observability::tui::register(
            "web service",
            [
                format!("http://127.0.0.1:{port}"),
                format!("broker {broker_backend_display} via {broker_connection}"),
            ],
        );
        runinator_observability::tui::gauge(
            "web service",
            "HTTP capacity",
            max_concurrent_requests as i64,
        );
        let dashboard_shutdown = shutdown.clone();
        let dashboard_stop = dashboard_shutdown.clone();
        runinator_observability::tui::spawn(
            dashboard,
            move || dashboard_shutdown.is_cancelled(),
            move || dashboard_stop.trigger(),
        );
    }
    let notify = shutdown.notifier();
    let broker = resources
        .broker()
        .expect("web service requested broker")
        .clone();
    let blobs = resources
        .blobs()
        .expect("web service requested blob store")
        .clone();
    let database = resources
        .database()
        .expect("web service requested database");

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
    runinator_observability::tui::activity(
        "web service",
        format!("listening on port {port}"),
        None,
    );
    dispatch_server_database!(database, |db| {
        run_webserver(
            db,
            notify.clone(),
            port,
            broker.clone(),
            blobs.clone(),
            advertisement.clone(),
            auth_options.clone(),
            cors_options.clone(),
            rate_limit_options,
            overload_options,
            run_engine,
            max_concurrent_ingress,
        )
        .await?;
    });

    Ok(())
}

fn should_spawn_gossip_advertiser(disable_gossip: bool) -> bool {
    !disable_gossip
}

fn map_bootstrap_error(err: ServerBootstrapError) -> SendableError {
    Box::new(err)
}
