use std::{
    fs,
    net::TcpListener,
    path::{Path, PathBuf},
    process::Command,
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use runinator_api::{AsyncApiClient, StaticLocator};
use runinator_models::json;
use runinator_models::value::Value;
use runinator_models::workflows::{WorkflowNodeRun, WorkflowRun, WorkflowStatus};
use sqlx::Row;
use tokio::time::sleep;

type E2eResult<T> = Result<T, Box<dyn std::error::Error + Send + Sync>>;
type ApiClient = AsyncApiClient<StaticLocator>;

#[tokio::test]
#[ignore = "starts a local Runinator stack; run with RUNINATOR_E2E=1 cargo test -p runinator-e2e brokered_result_path_smoke -- --ignored"]
async fn brokered_result_path_smoke() -> E2eResult<()> {
    if std::env::var("RUNINATOR_E2E").ok().as_deref() != Some("1") {
        eprintln!("set RUNINATOR_E2E=1 to run local-stack e2e tests");
        return Ok(());
    }

    let workspace = workspace_dir();
    build_service_binaries(&workspace)?;

    let ports = Ports::allocate()?;
    let harness = StackHarness::start(&workspace, ports).await?;
    let api = harness.api_client()?;

    // import the workflow through the wdlp pack path, the same way the real stack ships workflows.
    harness.import_workflows(&workspace.join("runinator-e2e/fixtures/broker-smoke.wdlp"))?;
    let workflow = api
        .fetch_workflow_by_name("Brokered Result Path Smoke")
        .await?;
    let workflow_id = workflow.id.ok_or("imported smoke workflow has no id")?;

    let (_run, nodes) = run_workflow_by_id(&api, workflow_id, json!({})).await?;
    let action = latest_node(&nodes, "write_logs")?;
    assert_eq!(action.status, WorkflowStatus::Succeeded);
    assert_eq!(
        action
            .output_json
            .as_ref()
            .and_then(|value| value.get("success")),
        Some(&Value::Bool(true))
    );

    let chunks = poll_node_chunks(&api, action.id).await?;
    let log = chunks
        .iter()
        .map(|chunk| chunk.content.as_str())
        .collect::<Vec<_>>()
        .join("\n");
    assert!(
        log.contains("broker-smoke-start"),
        "missing streamed stdout chunk: {log}"
    );
    assert!(
        log.contains("broker-smoke-end"),
        "missing streamed stdout chunk: {log}"
    );

    assert_broker_result_events(&harness.sqlite_path, action.id).await?;
    Ok(())
}

async fn run_workflow_by_id(
    api: &ApiClient,
    workflow_id: i64,
    parameters: Value,
) -> E2eResult<(WorkflowRun, Vec<WorkflowNodeRun>)> {
    let run = api.create_workflow_run(workflow_id, parameters).await?;
    poll_workflow(api, run.id).await
}

async fn poll_workflow(
    api: &ApiClient,
    workflow_run_id: i64,
) -> E2eResult<(WorkflowRun, Vec<WorkflowNodeRun>)> {
    for _ in 0..60 {
        let detail = api.fetch_workflow_run(workflow_run_id).await?;
        if detail.0.status.is_terminal() {
            if detail.0.status == WorkflowStatus::Succeeded {
                return Ok(detail);
            }
            return Err(format!(
                "workflow run {workflow_run_id} finished with status {}",
                detail.0.status.as_str()
            )
            .into());
        }
        sleep(Duration::from_secs(2)).await;
    }
    Err(format!("workflow run {workflow_run_id} did not finish in time").into())
}

fn latest_node<'a>(nodes: &'a [WorkflowNodeRun], node_id: &str) -> E2eResult<&'a WorkflowNodeRun> {
    nodes
        .iter()
        .filter(|node| node.node_id == node_id)
        .max_by_key(|node| node.id)
        .ok_or_else(|| format!("missing node run {node_id}").into())
}

async fn poll_node_chunks(
    api: &ApiClient,
    workflow_node_run_id: i64,
) -> E2eResult<Vec<runinator_models::workflows::WorkflowNodeRunChunk>> {
    for _ in 0..30 {
        let chunks = api
            .fetch_workflow_node_run_chunks(workflow_node_run_id, None, 100)
            .await?;
        if !chunks.is_empty() {
            return Ok(chunks);
        }
        sleep(Duration::from_secs(1)).await;
    }
    Err(format!("workflow node run {workflow_node_run_id} did not receive chunks").into())
}

async fn assert_broker_result_events(
    sqlite_path: &Path,
    workflow_node_run_id: i64,
) -> E2eResult<()> {
    let url = format!("sqlite://{}", sqlite_path.display());
    let pool = sqlx::SqlitePool::connect(&url).await?;
    let rows = sqlx::query(
        "SELECT event_type, COUNT(*) AS count FROM workflow_result_events WHERE workflow_node_run_id = ? GROUP BY event_type",
    )
    .bind(workflow_node_run_id)
    .fetch_all(&pool)
    .await?;

    let mut chunk_count = 0_i64;
    let mut status_count = 0_i64;
    for row in rows {
        match row.get::<String, _>("event_type").as_str() {
            "chunk" => chunk_count = row.get("count"),
            "status" => status_count = row.get("count"),
            _ => {}
        }
    }

    assert!(
        chunk_count >= 2,
        "expected broker result consumer to persist streamed chunks, got {chunk_count}"
    );
    assert!(
        status_count >= 2,
        "expected broker result consumer to persist running and final statuses, got {status_count}"
    );
    Ok(())
}

struct StackHarness {
    workspace: PathBuf,
    config_path: PathBuf,
    sqlite_path: PathBuf,
    api_url: String,
}

impl StackHarness {
    async fn start(workspace: &Path, ports: Ports) -> E2eResult<Self> {
        let run_dir = workspace
            .join("target")
            .join("e2e")
            .join(format!("stack-smoke-{}", unique_suffix()));
        fs::create_dir_all(&run_dir)?;
        let config_path = run_dir.join("supervisor.json");
        let target_debug = workspace.join("target/debug");
        let sqlite_path = run_dir.join("runinator.db");
        let state_dir = run_dir.join("supervisor-state");

        let config = json!({
            "state_dir": state_dir,
            "shutdown_timeout_secs": 12,
            "restart_delay_ms": 1000,
            "processes": [
                {
                    "name": "broker",
                    "command": target_debug.join(bin_name("runinator-broker")),
                    "env": {
                        "RUNINATOR_BROKER_ADDR": format!("127.0.0.1:{}", ports.broker)
                    }
                },
                {
                    "name": "web-service",
                    "command": target_debug.join(bin_name("runinator-ws")),
                    "args": [
                        "--database", "sqlite",
                        "--sqlite-path", sqlite_path,
                        "--port", ports.web.to_string(),
                        "--broker-backend", "tcp",
                        "--broker-endpoint", format!("127.0.0.1:{}", ports.broker),
                        "--gossip-bind", "127.0.0.1",
                        "--gossip-port", ports.web_gossip.to_string(),
                        "--gossip-targets", format!("127.0.0.1:{}", ports.scheduler_gossip),
                        "--announce-address", "127.0.0.1",
                        "--announce-base-path", "/",
                        "--gossip-interval-seconds", "1"
                    ]
                },
                {
                    "name": "waker",
                    "command": target_debug.join(bin_name("runinator-waker")),
                    "args": [
                        "--broker-backend", "tcp",
                        "--broker-endpoint", format!("127.0.0.1:{}", ports.broker)
                    ]
                },
                {
                    "name": "worker",
                    "command": target_debug.join(bin_name("runinator-worker")),
                    "args": [
                        "--broker-backend", "tcp",
                        "--broker-endpoint", format!("127.0.0.1:{}", ports.broker),
                        "--api-base-url", format!("http://127.0.0.1:{}/", ports.web),
                        "--max-concurrent-actions", "1"
                    ]
                }
            ]
        });
        fs::write(&config_path, serde_json::to_vec_pretty(&config)?)?;

        let harness = Self {
            workspace: workspace.to_path_buf(),
            config_path,
            sqlite_path,
            api_url: format!("http://127.0.0.1:{}/", ports.web),
        };
        harness.supervisor("start")?;
        harness.wait_for_web().await?;
        // the real stack registers provider/action metadata through the importer; without it,
        // workflow validation rejects every action node (e.g. unknown provider action 'Console.run').
        harness.register_providers()?;
        Ok(harness)
    }

    /// run the importer once to register the built-in provider bundle with the web service. mirrors
    /// the importer process in the supervisor stack so action nodes pass workflow validation.
    fn register_providers(&self) -> E2eResult<()> {
        // provider registration is independent of the workflows file, but the importer's --once mode
        // requires one to exist. import an empty bundle so we register providers without seeding.
        let seed = self
            .config_path
            .parent()
            .ok_or("supervisor config has no parent directory")?
            .join("provider-seed.json");
        fs::write(&seed, br#"{"workflows":[],"triggers":[]}"#)?;
        self.import_workflows(&seed)
    }

    /// run the importer once against the given workflows file (a .json bundle, .wdl file, or .wdlp
    /// pack). registers the built-in provider bundle and imports the workflows it resolves.
    fn import_workflows(&self, workflows_file: &Path) -> E2eResult<()> {
        let status = Command::new(
            self.workspace
                .join("target/debug")
                .join(bin_name("runinator-importer")),
        )
        .args(["--once", "--api-base-url", &self.api_url])
        .arg("--workflows-file")
        .arg(workflows_file)
        .current_dir(&self.workspace)
        .status()?;
        if status.success() {
            Ok(())
        } else {
            Err(format!("runinator-importer failed with {status}").into())
        }
    }

    fn api_client(&self) -> reqwest::Result<ApiClient> {
        AsyncApiClient::new(StaticLocator::new(self.api_url.clone()))
    }

    async fn wait_for_web(&self) -> E2eResult<()> {
        let client = reqwest::Client::builder()
            .timeout(Duration::from_secs(2))
            .build()?;
        for _ in 0..60 {
            match client
                .get(format!("{}providers", self.api_url))
                .send()
                .await
            {
                Ok(response) if response.status().is_success() => return Ok(()),
                _ => sleep(Duration::from_secs(1)).await,
            }
        }
        Err("web service did not become ready".into())
    }

    fn supervisor(&self, command: &str) -> E2eResult<()> {
        let status = Command::new(
            self.workspace
                .join("target/debug")
                .join(bin_name("runinator-supervisor")),
        )
        .arg("--config")
        .arg(&self.config_path)
        .arg(command)
        .current_dir(&self.workspace)
        .status()?;
        if status.success() {
            Ok(())
        } else {
            Err(format!("runinator-supervisor {command} failed with {status}").into())
        }
    }
}

impl Drop for StackHarness {
    fn drop(&mut self) {
        let _ = self.supervisor("stop");
    }
}

#[derive(Debug, Clone, Copy)]
struct Ports {
    broker: u16,
    web: u16,
    web_gossip: u16,
    scheduler_gossip: u16,
}

impl Ports {
    fn allocate() -> std::io::Result<Self> {
        Ok(Self {
            broker: free_port()?,
            web: free_port()?,
            web_gossip: free_port()?,
            scheduler_gossip: free_port()?,
        })
    }
}

fn free_port() -> std::io::Result<u16> {
    let listener = TcpListener::bind("127.0.0.1:0")?;
    Ok(listener.local_addr()?.port())
}

fn build_service_binaries(workspace: &Path) -> E2eResult<()> {
    let status = Command::new("cargo")
        .args([
            "build",
            "-p",
            "runinator-supervisor",
            "-p",
            "runinator-broker",
            "-p",
            "runinator-ws",
            "-p",
            "runinator-waker",
            "-p",
            "runinator-worker",
            "-p",
            "runinator-importer",
        ])
        .current_dir(workspace)
        .status()?;
    if status.success() {
        Ok(())
    } else {
        Err(format!("cargo build for e2e services failed with {status}").into())
    }
}

fn workspace_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("e2e crate has a workspace parent")
        .to_path_buf()
}

fn unique_suffix() -> String {
    let millis = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("system clock before unix epoch")
        .as_millis();
    format!("{}-{millis}", std::process::id())
}

fn bin_name(name: &str) -> String {
    if cfg!(windows) {
        format!("{name}.exe")
    } else {
        name.into()
    }
}
