mod config;
mod discovery;

use std::{path::Path, time::SystemTime};

use chrono::{DateTime, Utc};
use clap::Parser;
use config::Config;
use discovery::ServiceDiscovery;
use log::{error, info};
use runinator_api::AsyncApiClient;
use runinator_models::core::ScheduledTask;
use serde::Deserialize;
use tokio::time::{self, Duration};

type DynError = Box<dyn std::error::Error + Send + Sync>;
type ApiClient = AsyncApiClient<ServiceDiscovery>;

#[derive(Deserialize)]
struct TaskFile {
    tasks: Vec<TaskDefinition>,
}

#[derive(Deserialize)]
struct TaskDefinition {
    id: i64,
    name: String,
    cron_schedule: String,
    action_name: String,
    action_function: String,
    action_configuration: String,
    timeout: i64,
    #[serde(default = "default_enabled")]
    enabled: bool,
    #[serde(default)]
    immediate: bool,
    #[serde(default)]
    next_execution: Option<DateTime<Utc>>,
    #[serde(default)]
    blackout_start: Option<DateTime<Utc>>,
    #[serde(default)]
    blackout_end: Option<DateTime<Utc>>,
}

fn default_enabled() -> bool {
    true
}

impl From<TaskDefinition> for ScheduledTask {
    fn from(def: TaskDefinition) -> Self {
        ScheduledTask {
            id: Some(def.id),
            name: def.name,
            cron_schedule: def.cron_schedule,
            action_name: def.action_name,
            action_function: def.action_function,
            action_configuration: def.action_configuration,
            timeout: def.timeout,
            next_execution: def.next_execution,
            enabled: def.enabled,
            immediate: def.immediate,
            blackout_start: def.blackout_start,
            blackout_end: def.blackout_end,
        }
    }
}

#[tokio::main]
async fn main() -> Result<(), DynError> {
    env_logger::init();
    let config = Config::parse();

    info!("Starting Runinator Importer");
    let discovery = ServiceDiscovery::new(&config).await?;
    let http_client = reqwest::Client::builder()
        .timeout(Duration::from_secs(30))
        .build()?;
    let api = ApiClient::with_client(discovery.clone(), http_client);

    let mut interval = time::interval(Duration::from_secs(config.poll_interval_seconds.max(1)));
    let mut last_modified: Option<SystemTime> = None;

    loop {
        tokio::select! {
            _ = interval.tick() => {
                if let Err(err) = sync_tasks_if_changed(&config, &api, &mut last_modified).await {
                    error!("Failed to synchronize tasks: {}", err);
                }
            }
            _ = tokio::signal::ctrl_c() => {
                info!("Shutdown signal received. Exiting importer.");
                break;
            }
        }
    }

    Ok(())
}

async fn sync_tasks_if_changed(
    config: &Config,
    api: &ApiClient,
    last_modified: &mut Option<SystemTime>,
) -> Result<(), DynError> {
    let path = Path::new(&config.tasks_file);
    let metadata = tokio::fs::metadata(path).await?;
    let modified = metadata.modified()?;

    let should_sync = last_modified.map_or(true, |prev| modified > prev);
    if !should_sync {
        return Ok(());
    }

    let tasks = load_tasks(path).await?;
    info!("Seeding {} task(s) from {}", tasks.len(), path.display());
    for task in tasks {
        let _ = api
            .upsert_task(&task)
            .await
            .map_err(|err| -> DynError { Box::new(err) })?;
    }

    *last_modified = Some(modified);
    Ok(())
}

async fn load_tasks(path: &Path) -> Result<Vec<ScheduledTask>, DynError> {
    let data = tokio::fs::read_to_string(path).await?;
    let parsed: TaskFile = serde_json::from_str(&data)?;
    Ok(parsed.tasks.into_iter().map(Into::into).collect())
}

