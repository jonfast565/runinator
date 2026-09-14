use std::{net::Ipv4Addr, path::PathBuf, sync::Arc, time::Duration};

use clap::ValueEnum;
use runinator_adapter_client::HttpAdapterHostClient;
use runinator_blob::{BlobStore, FsBlobStore};
use runinator_broker::{Broker, BrokerClientConfig, BrokerConsumerProfile, build_broker_client};
use runinator_database::{BootstrapOptions, bootstrap_database};
use runinator_db_cli::{
    DatabaseBackend, dispatch_database, prepare_sqlite_path, required_database_url,
};
use runinator_engine::BackgroundEngineStore;
use runinator_models::{errors::SendableError, provisioning::NodeSpec, replicas::ReplicaKind};
use runinator_platform::startup::Shutdown;
use runinator_provisioner::{Provisioner, ProvisionerRegistry};
use runinator_worker::{AgentRuntime, NoopObserver};
use runinator_ws::{
    AuthOptions, CircuitBreakerConfig, CorsConfig, OverloadConfig, RateLimitConfig,
    ReplicaAdvertisement, WebserverRuntime, run_webserver,
};
use tokio::{net::TcpListener, task::JoinHandle};

use crate::{
    config::StandaloneConfig, provisioner::StandaloneProvisioner,
    runtime_factory::StandaloneRuntimeFactory, state::StateTracker,
};

const LOCAL_API_KEY: &str = "localdev.runinator-local-dev-service-key";
const ADAPTER_TOKEN: &str = "localdev.runinator-local-dev-adapter-host-token";

pub async fn run(
    config: StandaloneConfig,
    tracker: StateTracker,
    shutdown: Shutdown,
) -> Result<(), SendableError> {
    let backend = DatabaseBackend::from_str(&config.database, true)
        .map_err(|error| -> SendableError { error.into() })?;
    let sqlite = prepare_sqlite_path(config.sqlite_path.clone()).await?;
    let url = match backend {
        DatabaseBackend::Sqlite => String::new(),
        DatabaseBackend::Postgres | DatabaseBackend::Mariadb => {
            required_database_url(config.database_url.clone())?
        }
    };
    dispatch_database!(backend, sqlite: sqlite, url: url, |db| {
        bootstrap_database(&db, &BootstrapOptions {
            auth_bootstrap_admin: Some("admin:admin".into()),
            auth_bootstrap_service_api_key: Some(LOCAL_API_KEY.into()),
            auth_bootstrap_service_api_key_name: Some("local-standalone".into()),
            ..Default::default()
        }).await?;
        run_with_database(db, config, tracker, shutdown).await
    })
}

async fn run_with_database<T>(
    db: Arc<T>,
    config: StandaloneConfig,
    tracker: StateTracker,
    shutdown: Shutdown,
) -> Result<(), SendableError>
where
    T: BackgroundEngineStore + runinator_store::DatabaseImpl + 'static,
{
    let shutdown_notify = shutdown.notifier();
    let broker_listener = TcpListener::bind((Ipv4Addr::LOCALHOST, config.broker_port)).await?;
    let blob_listener = TcpListener::bind((Ipv4Addr::LOCALHOST, config.blob_port)).await?;
    let adapter_listener = TcpListener::bind((Ipv4Addr::LOCALHOST, config.adapter_port)).await?;
    let web_listener = TcpListener::bind((Ipv4Addr::UNSPECIFIED, config.api_port)).await?;

    let memory_broker = runinator_broker::in_memory::InMemoryBroker::new();
    let broker: Arc<dyn Broker> = build_broker_client(
        &broker_config(&config, "runinator-standalone"),
        BrokerConsumerProfile::WorkflowRuntime,
    )
    .await
    .map_err(|error| -> SendableError { Box::new(error) })?;

    let blob_config = runinator_blob::BlobServerConfig::from_env()?;
    let blob_store = FsBlobStore::open(&blob_config.data_dir).await?;
    for bucket in runinator_blob::REQUIRED_BUCKETS {
        blob_store.create_bucket(bucket).await?;
    }
    let blob_service = Arc::new(runinator_blob::server::BlobService::new(
        Arc::new(blob_store),
        blob_config,
    ));
    let blobs = runinator_blob::from_env().await?;

    let adapter: Arc<dyn runinator_adapter_client::AdapterPoller> =
        Arc::new(HttpAdapterHostClient::new(
            format!("http://127.0.0.1:{}", config.adapter_port),
            Some(ADAPTER_TOKEN.into()),
        ));
    let factory = Arc::new(StandaloneRuntimeFactory {
        db: db.clone(),
        broker: broker.clone(),
        adapter,
        config: config.clone(),
        tracker: tracker.clone(),
    });
    let provisioner = Arc::new(StandaloneProvisioner::new(factory.clone(), tracker.clone()));
    let registry = Arc::new(ProvisionerRegistry::new(vec![provisioner.clone()]));
    let supervision_task = tokio::spawn(provisioner.clone().supervise(shutdown_notify.clone()));

    let mut services: Vec<(String, JoinHandle<Result<(), String>>)> = Vec::new();
    tracker.starting("broker", "broker");
    services.push((
        "broker".into(),
        tokio::spawn(async move {
            runinator_broker::tcp::server::serve(broker_listener, memory_broker)
                .await
                .map_err(|error| error.to_string())
        }),
    ));
    tracker.running("broker");

    tracker.starting("blob", "blob");
    let blob_shutdown = shutdown_notify.clone();
    services.push((
        "blob".into(),
        tokio::spawn(async move {
            axum::serve(blob_listener, runinator_blob::server::router(blob_service))
                .with_graceful_shutdown(async move { blob_shutdown.notified().await })
                .await
                .map_err(|error| error.to_string())
        }),
    ));
    tracker.running("blob");

    tracker.starting("adapter-host", "adapter");
    let adapter_shutdown = shutdown_notify.clone();
    let plugin_paths = plugin_paths();
    services.push((
        "adapter-host".into(),
        tokio::spawn(async move {
            runinator_adapter_host::serve(
                adapter_listener,
                ADAPTER_TOKEN.into(),
                plugin_paths,
                async move { adapter_shutdown.notified().await },
            )
            .await
            .map_err(|error| error.to_string())
        }),
    ));
    tracker.running("adapter-host");

    tracker.starting("web-service", "webservice");
    let web_shutdown = shutdown_notify.clone();
    let web_config = config.clone();
    services.push((
        "web-service".into(),
        tokio::spawn(async move {
            run_webserver(WebserverRuntime {
                pool: db,
                notify: web_shutdown,
                port: web_config.api_port,
                listener: Some(web_listener),
                broker,
                blobs,
                advertisement: ReplicaAdvertisement {
                    instance_id: Some("runinator-standalone-web".into()),
                    host: Some("127.0.0.1".into()),
                    attributes: runinator_models::json!({
                        "broker_backend": "tcp",
                        "broker_connection": "standalone",
                        "database_backend": web_config.database,
                    }),
                },
                auth: AuthOptions {
                    enabled: web_config.auth_enabled,
                    ..Default::default()
                },
                cors: CorsConfig::default(),
                rate_limit: RateLimitConfig::default(),
                circuit_breaker: CircuitBreakerConfig::default(),
                overload: OverloadConfig::default(),
                provisioner: Some(registry),
                run_engine: false,
                max_concurrent_ingress: web_config.engine_concurrency,
                workspace_limits: Default::default(),
            })
            .await
            .map_err(|error| error.to_string())
        }),
    ));
    tracker.running("web-service");

    provisioner
        .scale(
            ReplicaKind::Background,
            config.engines,
            &NodeSpec::default(),
        )
        .await?;
    provisioner
        .scale(ReplicaKind::Waker, config.wakers, &NodeSpec::default())
        .await?;
    provisioner
        .scale(ReplicaKind::Worker, config.workers, &NodeSpec::default())
        .await?;
    let desktop = if config.desktop_agent {
        Some(start_desktop_agent(factory.as_ref(), tracker.clone()).await?)
    } else {
        None
    };

    let snapshot_path = config.state_dir.join("state.json");
    let snapshot_tracker = tracker.clone();
    let snapshot_shutdown = shutdown.clone();
    let snapshot_task = tokio::spawn(async move {
        let mut interval = tokio::time::interval(Duration::from_millis(500));
        loop {
            tokio::select! {
                _ = snapshot_shutdown.cancelled() => return,
                _ = interval.tick() => { let _ = snapshot_tracker.write(&snapshot_path); }
            }
        }
    });

    wait_for_api(&config, &tracker, &shutdown).await;
    if !shutdown.is_cancelled() {
        start_pack_hooks(&config, &tracker).await;
    }
    let stop_file = config.state_dir.join("stop");
    let result = loop {
        if stop_file.exists() {
            shutdown.trigger();
            break Ok(());
        }
        if let Some((name, _task)) = services.iter().find(|(_, task)| task.is_finished()) {
            shutdown.trigger();
            break Err(format!("standalone singleton '{name}' exited unexpectedly").into());
        }
        tokio::select! {
            _ = shutdown.cancelled() => break Ok(()),
            _ = tokio::time::sleep(Duration::from_millis(250)) => {}
        }
    };

    provisioner.shutdown_all().await;
    if let Some((mut handle, id)) = desktop {
        let _ = handle.stop(Duration::from_secs(35)).await;
        tracker.stopped(&id);
    }
    shutdown.trigger();
    let _ = supervision_task.await;
    for (_, task) in services {
        task.abort();
        let _ = task.await;
    }
    snapshot_task.abort();
    let _ = snapshot_task.await;
    let _ = tracker.write(&config.state_dir.join("state.json"));
    result
}

async fn wait_for_api(config: &StandaloneConfig, tracker: &StateTracker, shutdown: &Shutdown) {
    tracker.starting("api-readiness", "startup-hook");
    let client = reqwest::Client::new();
    let url = format!("http://127.0.0.1:{}/openapi.json", config.api_port);
    for _ in 0..120 {
        if client
            .get(&url)
            .send()
            .await
            .is_ok_and(|response| response.status().is_success())
        {
            tracker.running("api-readiness");
            tracker.stopped("api-readiness");
            return;
        }
        tokio::select! {
            _ = shutdown.cancelled() => return,
            _ = tokio::time::sleep(Duration::from_millis(250)) => {}
        }
    }
    tracker.failed(
        "api-readiness",
        format!("web service did not become ready at {url}"),
    );
}

async fn start_desktop_agent<T>(
    factory: &StandaloneRuntimeFactory<T>,
    tracker: StateTracker,
) -> Result<(runinator_worker::AgentHandle, String), SendableError>
where
    T: BackgroundEngineStore + 'static,
{
    let id = "desktop-agent-headless".to_string();
    tracker.starting(&id, "worker");
    let mut runtime = factory.desktop_worker_config(&id)?;
    let node_dir = runinator_platform::app_data::app_data_path("standalone/nodes/desktop")?;
    std::fs::create_dir_all(&node_dir)?;
    runtime.credential_file = node_dir.join("credential.json");
    runtime.outbox_file = node_dir.join("result-outbox.jsonl");
    runtime.liveness_file.clear();
    runinator_worker::prepare_agent_credentials(&mut runtime).await?;
    let handle = AgentRuntime::start(runtime, Arc::new(NoopObserver))?;
    tracker.running(&id);
    Ok((handle, id))
}

async fn start_pack_hooks(config: &StandaloneConfig, tracker: &StateTracker) {
    if config.packs.is_empty() {
        return;
    }
    tracker.starting("pack-import", "startup-hook");
    for pack in &config.packs {
        let Some(executable) = sibling_executable("runinatorctl") else {
            tracker.failed(
                "pack-import",
                "runinatorctl was not found beside runinator-standalone",
            );
            return;
        };
        let mut command = tokio::process::Command::new(executable);
        command
            .env("RUNINATOR_API_KEY", LOCAL_API_KEY)
            .arg("--api-base-url")
            .arg(format!("http://127.0.0.1:{}/", config.api_port))
            .args(["workflows", "apply"])
            .arg(pack);
        match command.status().await {
            Ok(status) if status.success() => {}
            Ok(status) => {
                tracker.failed(
                    "pack-import",
                    format!("workflow import exited with {status}"),
                );
                return;
            }
            Err(error) => {
                tracker.failed("pack-import", error);
                return;
            }
        }
    }
    tracker.running("pack-import");
    tracker.stopped("pack-import");
}

fn sibling_executable(name: &str) -> Option<PathBuf> {
    let mut path = std::env::current_exe().ok()?;
    path.set_file_name(if cfg!(windows) {
        format!("{name}.exe")
    } else {
        name.into()
    });
    path.exists().then_some(path)
}

fn broker_config(config: &StandaloneConfig, client_id: &str) -> BrokerClientConfig {
    BrokerClientConfig {
        backend: "tcp".into(),
        endpoint: format!("127.0.0.1:{}", config.broker_port),
        control_topic: "runinator.control".into(),
        agent_topic: Some("runinator.agent".into()),
        effect_topic: "runinator.effects".into(),
        infrastructure_effect_topic: "runinator.effects.infrastructure".into(),
        effect_result_topic: "runinator.effect-results".into(),
        client_id: client_id.into(),
        relay_credential: None,
        wake_topic: Some("runinator.wake".into()),
        ingress_topic: Some("runinator.ingress".into()),
    }
}

fn plugin_paths() -> Vec<PathBuf> {
    std::env::var_os("RUNINATOR_ADAPTER_PLUGIN_PATHS")
        .map(|value| std::env::split_paths(&value).collect())
        .unwrap_or_default()
}

pub fn configure_environment(config: &StandaloneConfig) -> Result<(), SendableError> {
    let child = std::env::current_exe()?;
    let blob_dir = config.state_dir.join("blobs");
    // safety: configuration is applied before the standalone multithreaded runtime starts.
    unsafe {
        std::env::set_var(
            "RUNINATOR_STANDALONE_STATE_PATH",
            config.state_dir.join("state.json"),
        );
        std::env::set_var("RUNINATOR_ADAPTER_CHILD_EXE", child);
        std::env::set_var(
            "RUNINATOR_ADAPTER_HOST_URL",
            format!("http://127.0.0.1:{}", config.adapter_port),
        );
        std::env::set_var("RUNINATOR_ADAPTER_HOST_TOKEN", ADAPTER_TOKEN);
        std::env::set_var(
            "RUNINATOR_BLOB_ADDR",
            format!("127.0.0.1:{}", config.blob_port),
        );
        std::env::set_var(
            "RUNINATOR_BLOB_ENDPOINT",
            format!("http://127.0.0.1:{}/", config.blob_port),
        );
        std::env::set_var("RUNINATOR_BLOB_DATA_DIR", blob_dir);
        std::env::set_var("RUNINATOR_BLOB_ALLOW_ANONYMOUS", "true");
        std::env::set_var(
            "RUNINATOR_SERVICE_URL",
            format!("http://127.0.0.1:{}/", config.api_port),
        );
        std::env::set_var("RUNINATOR_API_KEY", LOCAL_API_KEY);
        std::env::set_var("RUNINATOR_LOCAL_FILES_ROOT", ".");
    }
    Ok(())
}
