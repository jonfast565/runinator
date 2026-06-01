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
use runinator_models::workflows::{
    WorkflowDefinition, WorkflowNodeRun, WorkflowRun, WorkflowStatus,
};
use serde::Deserialize;
use sqlx::Row;
use tokio::time::sleep;

type E2eResult<T> = Result<T, Box<dyn std::error::Error + Send + Sync>>;
type ApiClient = AsyncApiClient<StaticLocator>;

#[tokio::test]
#[ignore = "starts a local Runinator stack; run with RUNINATOR_E2E=1 cargo test -p runinator-e2e -- --ignored"]
async fn rich_workflow_demo_paths_finish() -> E2eResult<()> {
    if std::env::var("RUNINATOR_E2E").ok().as_deref() != Some("1") {
        eprintln!("set RUNINATOR_E2E=1 to run local-stack e2e tests");
        return Ok(());
    }

    let workspace = workspace_dir();
    build_service_binaries(&workspace)?;

    let ports = Ports::allocate()?;
    let harness = StackHarness::start(&workspace, ports).await?;
    let api = harness.api_client()?;

    import_seed(
        &api,
        &workspace.join("runinator-importer/workflows/workflows.json"),
    )
    .await?;

    let default = run_workflow_case(&api, json!({})).await?;
    assert_node_status(&default.1, "guarded_release", WorkflowStatus::Succeeded);

    let batch = run_workflow_case(&api, json!({ "mode": "batch", "items": ["a", "b"] })).await?;
    let process_items = node_output(&batch.1, "process_items")?;
    assert_eq!(process_items["count"], 2);

    let race = run_workflow_case(&api, json!({ "mode": "race" })).await?;
    let first_ready = node_output(&race.1, "first_ready")?;
    assert_eq!(first_ready["winner"], "fast_signal");

    Ok(())
}

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
    let workflow = api.upsert_workflow(&broker_smoke_workflow()).await?;
    let workflow_id = workflow.id.ok_or("smoke workflow did not get an id")?;

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

async fn run_workflow_case(
    api: &ApiClient,
    parameters: Value,
) -> E2eResult<(WorkflowRun, Vec<WorkflowNodeRun>)> {
    run_workflow_by_id(api, 1002, parameters).await
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

async fn import_seed(api: &ApiClient, path: &Path) -> E2eResult<()> {
    let seed = load_import_file(path)?;
    for workflow in seed.workflows {
        api.upsert_workflow(&workflow).await?;
    }
    Ok(())
}

fn assert_node_status(nodes: &[WorkflowNodeRun], node_id: &str, status: WorkflowStatus) {
    let node = latest_node(nodes, node_id).unwrap_or_else(|err| panic!("{err}"));
    assert_eq!(node.status, status);
}

fn node_output(nodes: &[WorkflowNodeRun], node_id: &str) -> E2eResult<Value> {
    latest_node(nodes, node_id)?
        .output_json
        .clone()
        .ok_or_else(|| format!("missing output for node {node_id}").into())
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

fn broker_smoke_workflow() -> WorkflowDefinition {
    WorkflowDefinition {
        id: None,
        name: "brokered result path smoke".into(),
        version: 1,
        enabled: true,
        input_type: runinator_models::types::RuninatorType::Any,
        definition: runinator_models::workflows::WorkflowGraph::from_value(json!({
            "start": "start",
            "nodes": [
                {
                    "id": "start",
                    "kind": "start",
                    "transitions": { "next": { "$node": "write_logs" } }
                },
                {
                    "id": "write_logs",
                    "kind": "action",
                    "action": {
                        "provider": "Console",
                        "function": "run",
                        "timeout_seconds": 10,
                        "configuration": {
                            "command": "printf 'broker-smoke-start\\nbroker-smoke-end\\n'"
                        }
                    },
                    "transitions": { "on_success": { "$node": "done" } }
                },
                {
                    "id": "done",
                    "kind": "end"
                }
            ]
        }))
        .unwrap(),
        created_at: None,
        updated_at: None,
    }
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
            .join(format!("rich-workflows-{}", unique_suffix()));
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
                    "name": "scheduler",
                    "command": target_debug.join(bin_name("runinator-scheduler")),
                    "args": [
                        "--scheduler-frequency-seconds", "1",
                        "--gossip-bind", "127.0.0.1",
                        "--gossip-port", ports.scheduler_gossip.to_string(),
                        "--gossip-targets", format!("127.0.0.1:{}", ports.web_gossip),
                        "--api-timeout-seconds", "30",
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
        Ok(harness)
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
            "runinator-scheduler",
            "-p",
            "runinator-worker",
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

#[derive(Deserialize)]
struct ImportFile {
    #[serde(default)]
    workflows: Vec<WorkflowDefinition>,
}

struct ImportSeed {
    workflows: Vec<WorkflowDefinition>,
}

fn load_import_file(path: &Path) -> E2eResult<ImportSeed> {
    let data = fs::read_to_string(path)?;
    let parsed: ImportFile = serde_json::from_str(&data)?;
    Ok(ImportSeed {
        workflows: parsed.workflows,
    })
}
