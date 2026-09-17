use clap::Parser;
use runinator_models::errors::SendableError;
use runinator_platform::app_data;
use std::collections::BTreeMap;
use std::time::Duration;
use uuid::Uuid;

use crate::agent::{
    AgentRuntimeConfig, BrokerMode, BrokerSelection, DEFAULT_HEARTBEAT_INTERVAL, LocatorMode,
    RECONNECT_UNLIMITED,
};
use crate::provider_repository::default_provider_factory;

pub fn parse_config() -> Result<Config, SendableError> {
    let args = CliArgs::parse();
    // A non-UUID identity, such as a stable Kubernetes pod name, is folded into a deterministic UUID.
    // The same pod keeps the same replica identity across restarts; a fresh UUID is minted only when no
    // identity is supplied.
    let worker_id = match args.worker_id {
        Some(ref value) if !value.is_empty() => Uuid::parse_str(value)
            .unwrap_or_else(|_| Uuid::new_v5(&Uuid::NAMESPACE_DNS, value.as_bytes())),
        _ => Uuid::new_v4(),
    };

    let consumer_id = args.broker_consumer_id.unwrap_or_else(|| {
        if args.broker_backend == "kafka" {
            "runinator-workers".to_string()
        } else {
            worker_id.to_string()
        }
    });

    let broker_mode = BrokerMode::parse(&args.broker_mode).ok_or_else(|| {
        crate::errors::BROKER_UNKNOWN_BACKEND
            .error(format!("unknown --broker-mode '{}'", args.broker_mode))
    })?;
    let api_base_url = args
        .service_url
        .filter(|value| !value.trim().is_empty())
        .unwrap_or(args.api_base_url);

    Ok(Config {
        tui: args.tui,
        dll_paths: plugin_search_paths(args.dll_paths),
        broker_mode,
        broker_backend: args.broker_backend,
        broker_endpoint: args.broker_endpoint,
        broker_control_topic: args.broker_control_topic,
        broker_agent_topic: args.broker_agent_topic,
        broker_effect_topic: args.broker_effect_topic,
        broker_infrastructure_effect_topic: args.broker_infrastructure_effect_topic,
        broker_effect_result_topic: args.broker_effect_result_topic,
        broker_ingress_topic: args.broker_ingress_topic,
        broker_client_id: args.broker_client_id,
        broker_consumer_id: consumer_id,
        max_concurrent_actions: args.max_concurrent_actions.max(1),
        shutdown_grace_seconds: args.shutdown_grace_seconds.max(1),
        reconnect_max_attempts: args.reconnect_max_attempts,
        api_base_url,
        locator_mode: if args.discover {
            LocatorMode::Discover
        } else {
            LocatorMode::Static
        },
        gossip_bind: args.gossip_bind,
        gossip_port: args.gossip_port,
        api_key: args.api_key.filter(|value| !value.trim().is_empty()),
        enrollment_token: args
            .enrollment_token
            .filter(|value| !value.trim().is_empty()),
        worker_id,
        advertise_host: args.advertise_host.filter(|value| !value.trim().is_empty()),
        liveness_file: args.liveness_file,
        labels: parse_labels(args.labels.as_deref()),
    })
}

/// parse a `k=v,k=v` label string into a map; blank entries and entries without a `=` are skipped.
/// shared with `runinator-desktop-agent` so both surfaces accept the same label syntax.
pub fn parse_labels(raw: Option<&str>) -> BTreeMap<String, String> {
    let mut labels = BTreeMap::new();
    let Some(raw) = raw else {
        return labels;
    };
    for entry in raw.split(',') {
        let Some((key, value)) = entry.split_once('=') else {
            continue;
        };
        let key = key.trim();
        let value = value.trim();
        if key.is_empty() || value.is_empty() {
            continue;
        }
        labels.insert(key.to_string(), value.to_string());
    }
    labels
}

fn plugin_search_paths(mut paths: Vec<String>) -> Vec<String> {
    paths.push(default_dll_path());
    paths.sort();
    paths.dedup();
    paths
}

fn default_dll_path() -> String {
    app_data::app_data_path("plugins")
        .map(|path| path.to_string_lossy().into_owned())
        .unwrap_or_else(|_| "plugins".to_string())
}

mod config;
pub use config::Config;

mod cli_args;
use cli_args::CliArgs;
