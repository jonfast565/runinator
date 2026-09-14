use std::{collections::BTreeMap, future::Future, pin::Pin, sync::Arc, time::Duration};

use async_trait::async_trait;
use chrono::Utc;
use runinator_adapter_client::AdapterPoller;
use runinator_broker::{Broker, IngressMessage, UiEventPublisher};
use runinator_comm::WsIngressCommand;
use runinator_engine::{BackgroundEngineStore, EngineConfig, run_background_engine_with_adapter};
use runinator_models::{
    errors::SendableError,
    provisioning::NodeSpec,
    replicas::{ReplicaKind, ReplicaRegistrationRequest},
};
use runinator_provider_catalog::{StaticProvider, built_in_providers};
use runinator_provider_local_files::LocalProvider;
use runinator_worker::{AgentRuntime, BrokerMode, Config as WorkerConfig, NoopObserver};
use tokio::{sync::Notify, task::JoinHandle};
use uuid::Uuid;

use crate::{config::StandaloneConfig, provisioner::RuntimeFactory, state::StateTracker};

pub struct StandaloneRuntimeFactory<T> {
    pub db: Arc<T>,
    pub broker: Arc<dyn Broker>,
    pub adapter: Arc<dyn AdapterPoller>,
    pub config: StandaloneConfig,
    pub tracker: StateTracker,
}

#[async_trait]
impl<T> RuntimeFactory for StandaloneRuntimeFactory<T>
where
    T: BackgroundEngineStore + 'static,
{
    async fn spawn(
        &self,
        kind: ReplicaKind,
        node_id: String,
        spec: NodeSpec,
        shutdown: Arc<Notify>,
    ) -> Result<JoinHandle<()>, SendableError> {
        self.tracker.starting(&node_id, kind.as_str());
        let tracker = self.tracker.clone();
        let result: Pin<Box<dyn Future<Output = Result<(), SendableError>> + Send>> = match kind {
            ReplicaKind::Worker => Box::pin(self.spawn_worker(node_id.clone(), spec, shutdown)),
            ReplicaKind::Waker => Box::pin(self.spawn_waker(node_id.clone(), shutdown)),
            ReplicaKind::Background => Box::pin(self.spawn_engine(node_id.clone(), shutdown)),
            _ => return Err(format!("standalone cannot host {}", kind.as_str()).into()),
        };
        Ok(tokio::spawn(async move {
            match result.await {
                Ok(()) => tracker.stopped(&node_id),
                Err(error) => tracker.failed(&node_id, error),
            }
        }))
    }
}

impl<T> StandaloneRuntimeFactory<T>
where
    T: BackgroundEngineStore + 'static,
{
    fn spawn_worker(
        &self,
        node_id: String,
        spec: NodeSpec,
        shutdown: Arc<Notify>,
    ) -> impl std::future::Future<Output = Result<(), SendableError>> + Send + 'static {
        let config = self.worker_config(&node_id, spec.labels);
        let tracker = self.tracker.clone();
        async move {
            let mut runtime = config.agent_runtime_config()?;
            let node_dir =
                runinator_platform::app_data::app_data_path(format!("standalone/nodes/{node_id}"))?;
            std::fs::create_dir_all(&node_dir)?;
            runtime.credential_file = node_dir.join("credential.json");
            runtime.outbox_file = node_dir.join("result-outbox.jsonl");
            runtime.liveness_file.clear();
            runinator_worker::prepare_agent_credentials(&mut runtime).await?;
            let mut handle = AgentRuntime::start(runtime, Arc::new(NoopObserver))?;
            tracker.running(&node_id);
            tokio::select! {
                _ = shutdown.notified() => handle.stop(Duration::from_secs(35)).await,
                result = handle.wait() => result,
            }
        }
    }

    fn worker_config(&self, node_id: &str, labels: BTreeMap<String, String>) -> WorkerConfig {
        WorkerConfig {
            tui: false,
            dll_paths: Vec::new(),
            broker_mode: BrokerMode::Direct,
            broker_backend: "tcp".into(),
            broker_endpoint: format!("127.0.0.1:{}", self.config.broker_port),
            broker_control_topic: "runinator.control".into(),
            broker_agent_topic: "runinator.agent".into(),
            broker_effect_topic: "runinator.effects".into(),
            broker_infrastructure_effect_topic: "runinator.effects.infrastructure".into(),
            broker_effect_result_topic: "runinator.effect-results".into(),
            broker_ingress_topic: "runinator.ingress".into(),
            broker_client_id: node_id.into(),
            broker_consumer_id: node_id.into(),
            max_concurrent_actions: self.config.worker_concurrency,
            shutdown_grace_seconds: 30,
            reconnect_max_attempts: 0,
            api_base_url: format!("http://127.0.0.1:{}/", self.config.api_port),
            locator_mode: runinator_worker::agent::LocatorMode::Static,
            gossip_bind: "127.0.0.1".into(),
            gossip_port: 0,
            api_key: Some("localdev.runinator-local-dev-service-key".into()),
            enrollment_token: None,
            worker_id: Uuid::new_v5(&Uuid::NAMESPACE_DNS, node_id.as_bytes()),
            advertise_host: Some("127.0.0.1".into()),
            liveness_file: String::new(),
            labels,
        }
    }

    fn spawn_waker(
        &self,
        node_id: String,
        shutdown: Arc<Notify>,
    ) -> impl std::future::Future<Output = Result<(), SendableError>> + Send + 'static {
        let broker = self.broker.clone();
        let tracker = self.tracker.clone();
        async move {
            let config =
                runinator_waker::config::normalize_config(runinator_waker::config::Config {
                    tui: false,
                    waker_id: node_id.clone(),
                    waker_consumer_group: "runinator-waker".into(),
                    max_wake_sleep_seconds: 20,
                    max_concurrent_wakes: 32,
                    broker_backend: "tcp".into(),
                    broker_endpoint: String::new(),
                    broker_mode: "direct".into(),
                    service_url: None,
                    api_key: None,
                    broker_relay_path: runinator_broker::DEFAULT_BROKER_RELAY_PATH.into(),
                    broker_effect_topic: "runinator.effects".into(),
                    broker_infrastructure_effect_topic: "runinator.effects.infrastructure".into(),
                    broker_control_topic: "runinator.control".into(),
                    broker_effect_result_topic: "runinator.effect-results".into(),
                    broker_wake_topic: "runinator.wake".into(),
                    broker_ingress_topic: "runinator.ingress".into(),
                    broker_client_id: node_id.clone(),
                    advertise_host: "127.0.0.1".into(),
                    broker_heartbeat_seconds: 10,
                    liveness_file: String::new(),
                });
            let replica_id = Uuid::now_v7();
            let runtime_id = replica_id.to_string();
            let settings = runinator_waker::runtime_settings(&config);
            let attributes = runinator_waker::attributes_with_waker_settings(
                &runinator_models::json!({"broker_backend": "tcp", "broker_connection": "standalone"}),
                &settings,
            );
            runinator_waker::publish_replica_availability(
                broker.as_ref(),
                &config,
                replica_id,
                &runtime_id,
                attributes.clone(),
            )
            .await?;
            let heartbeat = runinator_waker::spawn_replica_heartbeat(
                broker.clone(),
                config.clone(),
                replica_id,
                runtime_id,
                attributes,
                settings.clone(),
                shutdown.clone(),
            );
            tracker.running(&node_id);
            runinator_waker::waker_loop_with_settings(
                broker,
                shutdown.clone(),
                config.waker_consumer_group,
                settings,
            )
            .await;
            let _ = heartbeat.await;
            Ok(())
        }
    }

    fn spawn_engine(
        &self,
        node_id: String,
        shutdown: Arc<Notify>,
    ) -> impl std::future::Future<Output = Result<(), SendableError>> + Send + 'static {
        let db = self.db.clone();
        let broker = self.broker.clone();
        let adapter = self.adapter.clone();
        let tracker = self.tracker.clone();
        let ingress = self.config.engine_concurrency;
        async move {
            let replica_id = Uuid::now_v7();
            let runtime_id = replica_id.to_string();
            publish_engine_presence(broker.as_ref(), replica_id, &runtime_id, &node_id, false)
                .await?;
            let heartbeat = spawn_engine_heartbeat(
                broker.clone(),
                replica_id,
                runtime_id,
                node_id.clone(),
                shutdown.clone(),
            );
            tracker.running(&node_id);
            let result = run_background_engine_with_adapter(
                db,
                broker.clone(),
                UiEventPublisher::new(broker),
                None,
                adapter,
                node_id,
                EngineConfig {
                    max_concurrent_ingress: ingress,
                    workspace_limits: Default::default(),
                },
                shutdown.clone(),
            )
            .await;
            shutdown.notify_waiters();
            let _ = heartbeat.await;
            result
        }
    }

    pub fn desktop_worker_config(
        &self,
        node_id: &str,
    ) -> Result<runinator_worker::AgentRuntimeConfig, SendableError> {
        let mut labels = BTreeMap::new();
        labels.insert("pool".into(), "desktop".into());
        labels.insert("runner".into(), "desktop".into());
        let mut runtime = self.worker_config(node_id, labels).agent_runtime_config()?;
        runtime.exclusive = true;
        runtime.consumer_id = None;
        runtime.publish_providers = true;
        runtime.use_server_worker_settings = false;
        runtime.stale_after = Duration::from_secs(90);
        runtime.providers = Arc::new(|| {
            let mut providers: Vec<StaticProvider> = built_in_providers();
            providers.push(Box::new(LocalProvider));
            providers
        });
        Ok(runtime)
    }
}

async fn publish_engine_presence(
    broker: &dyn Broker,
    replica_id: Uuid,
    runtime_id: &str,
    node_id: &str,
    offline: bool,
) -> Result<(), runinator_broker::BrokerError> {
    let command = if offline {
        WsIngressCommand::replica_offline(replica_id, runtime_id.to_string())
    } else {
        WsIngressCommand::replica_available(
            ReplicaRegistrationRequest {
                replica_id: Some(replica_id),
                replica_type: ReplicaKind::Background,
                instance_id: node_id.into(),
                runtime_id: runtime_id.into(),
                display_name: Some(node_id.into()),
                host: Some("127.0.0.1".into()),
                port: None,
                base_path: None,
                version: Some(env!("CARGO_PKG_VERSION").into()),
                attributes: runinator_models::json!({"broker_backend": "tcp", "broker_connection": "standalone"}),
            },
            Vec::new(),
        )
    };
    broker
        .publish_ingress(IngressMessage {
            dedupe_key: Some(command.dedupe_key()),
            command,
            enqueued_at: Utc::now(),
        })
        .await
}

fn spawn_engine_heartbeat(
    broker: Arc<dyn Broker>,
    replica_id: Uuid,
    runtime_id: String,
    node_id: String,
    shutdown: Arc<Notify>,
) -> JoinHandle<()> {
    tokio::spawn(async move {
        let mut ticker = tokio::time::interval(Duration::from_secs(10));
        loop {
            tokio::select! {
                _ = shutdown.notified() => {
                    let _ = publish_engine_presence(broker.as_ref(), replica_id, &runtime_id, &node_id, true).await;
                    return;
                }
                _ = ticker.tick() => {
                    let _ = publish_engine_presence(broker.as_ref(), replica_id, &runtime_id, &node_id, false).await;
                }
            }
        }
    })
}
