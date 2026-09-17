#[allow(unused_imports)]
use super::*;

pub(super) struct StackHarness {
    pub(super) workspace: PathBuf,
    pub(super) run_dir: PathBuf,
    pub(super) config_path: PathBuf,
    pub(super) sqlite_path: PathBuf,
    pub(super) api_url: String,
}

impl StackHarness {
    pub(super) async fn start(workspace: &Path, ports: Ports) -> E2eResult<Self> {
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
                    ],
                    "env": {
                        "RUNINATOR_HOME": run_dir.join("worker-home")
                    }
                }
            ]
        });
        fs::write(&config_path, serde_json::to_vec_pretty(&config)?)?;

        let harness = Self {
            workspace: workspace.to_path_buf(),
            run_dir,
            config_path,
            sqlite_path,
            api_url: format!("http://127.0.0.1:{}/", ports.web),
        };
        harness.bootstrap_database()?;
        harness.supervisor("start")?;
        harness.wait_for_web().await?;
        // the web service seeds built-in provider metadata on startup; without it, workflow
        // validation rejects every action node (e.g. unknown provider action 'Console.run'). wait
        // for the catalog seed to land before importing workflows.
        harness.wait_for_providers().await?;
        Ok(harness)
    }

    /// poll the web service until the built-in provider catalog has been seeded.
    pub(super) async fn wait_for_providers(&self) -> E2eResult<()> {
        let client = self.api_client()?;
        for _ in 0..60 {
            match client.fetch_providers().await {
                Ok(providers) if !providers.is_empty() => return Ok(()),
                _ => sleep(Duration::from_secs(1)).await,
            }
        }
        Err("provider catalog was not seeded in time".into())
    }

    /// apply one workflow source, retrying the brief SQLite writer contention possible at startup.
    pub(super) fn import_workflows(&self, workflows_file: &Path) -> E2eResult<()> {
        for attempt in 1..=5 {
            let status = Command::new(
                self.workspace
                    .join("target/debug")
                    .join(bin_name("runinatorctl")),
            )
            .args(["--api-base-url", &self.api_url, "workflows", "apply"])
            .arg(workflows_file)
            .current_dir(&self.workspace)
            .status()?;
            if status.success() {
                return Ok(());
            }
            if attempt < 5 {
                std::thread::sleep(Duration::from_millis(250));
            }
        }
        Err("runinatorctl workflows apply failed after 5 attempts".into())
    }

    pub(super) fn api_client(&self) -> reqwest::Result<ApiClient> {
        AsyncApiClient::new(StaticLocator::new(self.api_url.clone()))
    }

    pub(super) fn bootstrap_database(&self) -> E2eResult<()> {
        let status = Command::new(
            self.workspace
                .join("target/debug")
                .join(bin_name("runinator-bootstrap")),
        )
        .args(["--database", "sqlite", "--database-url"])
        .arg(&self.sqlite_path)
        .current_dir(&self.workspace)
        .status()?;
        if status.success() {
            Ok(())
        } else {
            Err(format!("runinator-bootstrap failed with {status}").into())
        }
    }

    pub(super) async fn wait_for_web(&self) -> E2eResult<()> {
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

    pub(super) fn supervisor(&self, command: &str) -> E2eResult<()> {
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

    pub(super) fn supervisor_process(&self, command: &str, name: &str) -> E2eResult<()> {
        let status = Command::new(
            self.workspace
                .join("target/debug")
                .join(bin_name("runinator-supervisor")),
        )
        .arg("--config")
        .arg(&self.config_path)
        .args(["process", command, name])
        .current_dir(&self.workspace)
        .status()?;
        if status.success() {
            Ok(())
        } else {
            Err(format!("runinator-supervisor process {command} failed with {status}").into())
        }
    }
}

impl Drop for StackHarness {
    fn drop(&mut self) {
        let _ = self.supervisor("stop");
    }
}
