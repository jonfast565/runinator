#[allow(unused_imports)]
use super::*;

#[derive(Debug, Clone)]
pub struct Config {
    pub tui: bool,
    pub dll_paths: Vec<String>,
    /// how this worker reaches the broker. `Direct` uses `broker_backend`/`broker_endpoint` as
    /// given; `Relay` ignores both and tunnels through the web service instead.
    pub broker_mode: BrokerMode,
    pub broker_backend: String,
    pub broker_endpoint: String,
    pub broker_control_topic: String,
    pub broker_agent_topic: String,
    pub broker_effect_topic: String,
    pub broker_infrastructure_effect_topic: String,
    pub broker_effect_result_topic: String,
    pub broker_ingress_topic: String,
    pub broker_client_id: String,
    pub broker_consumer_id: String,
    pub max_concurrent_actions: usize,
    pub shutdown_grace_seconds: u64,
    /// consecutive reconnect failures tolerated before the worker stops itself; `0` retries forever.
    pub reconnect_max_attempts: u32,
    pub api_base_url: String,
    pub locator_mode: LocatorMode,
    pub gossip_bind: String,
    pub gossip_port: u16,
    pub api_key: Option<String>,
    pub enrollment_token: Option<String>,
    pub worker_id: Uuid,
    pub advertise_host: Option<String>,
    pub liveness_file: String,
    /// routing labels this worker advertises; the engine pins label-targeted effects to a worker
    /// whose labels are a superset of the action's required selector.
    pub labels: BTreeMap<String, String>,
}

impl Config {
    /// map this worker's CLI config onto the shared agent lifecycle.
    ///
    /// the worker is a general-pool, non-exclusive replica: it takes untargeted work, keeps the
    /// explicitly configured broker consumer id (kafka's shared group depends on it), and does not
    /// publish provider metadata — an in-cluster deployment has many identical workers and the extra
    /// round trips buy nothing.
    pub fn agent_runtime_config(&self) -> Result<AgentRuntimeConfig, SendableError> {
        let (broker, broker_description) = BrokerSelection {
            mode: self.broker_mode,
            service_url: self.api_base_url.clone(),
            direct_backend: self.broker_backend.clone(),
            direct_endpoint: self.broker_endpoint.clone(),
            control_topic: self.broker_control_topic.clone(),
            agent_topic: self.broker_agent_topic.clone(),
            effect_topic: self.broker_effect_topic.clone(),
            infrastructure_effect_topic: self.broker_infrastructure_effect_topic.clone(),
            effect_result_topic: self.broker_effect_result_topic.clone(),
            ingress_topic: self.broker_ingress_topic.clone(),
            client_id: self.broker_client_id.clone(),
            api_key: self.api_key.clone(),
        }
        .resolve()?;

        Ok(AgentRuntimeConfig {
            service_url: self.api_base_url.clone(),
            locator_mode: self.locator_mode,
            gossip_bind: self.gossip_bind.clone(),
            gossip_port: self.gossip_port,
            api_key: self.api_key.clone(),
            enrollment_token: self.enrollment_token.clone(),
            credential_file: app_data::app_data_path("agent/worker-credential.json")
                .unwrap_or_else(|_| std::path::PathBuf::from("worker-credential.json")),
            outbox_file: app_data::app_data_path("agent/worker-result-outbox.jsonl")
                .unwrap_or_else(|_| std::path::PathBuf::from("worker-result-outbox.jsonl")),
            instance_id: self.worker_id.to_string(),
            display_name: Some(format!("worker-{}", self.worker_id)),
            advertise_host: self.advertise_host.clone(),
            version: Some(env!("CARGO_PKG_VERSION").to_string()),
            labels: self.labels.clone(),
            exclusive: false,
            consumer_id: Some(self.broker_consumer_id.clone()),
            attributes: runinator_models::json!({
                "broker_client_id": self.broker_client_id,
                "broker_consumer_id": self.broker_consumer_id,
            }),
            broker,
            broker_description,
            providers: default_provider_factory(),
            publish_providers: false,
            dll_paths: self.dll_paths.clone(),
            max_concurrent_actions: self.max_concurrent_actions,
            shutdown_grace: Duration::from_secs(self.shutdown_grace_seconds),
            use_server_worker_settings: true,
            worker_settings_refresh_interval: Duration::from_secs(5),
            liveness_file: self.liveness_file.clone(),
            heartbeat_interval: DEFAULT_HEARTBEAT_INTERVAL,
            stale_after: Duration::from_secs(30),
            reconnect_max_attempts: self.reconnect_max_attempts,
            sample_telemetry: true,
            directive_handler: std::sync::Arc::new(crate::agent::DefaultDirectiveHandler),
        })
    }
}
