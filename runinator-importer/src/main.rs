mod config;
#[cfg(test)]
mod tests;

use std::{convert::Infallible, path::Path, time::SystemTime};

use async_trait::async_trait;
use clap::Parser;
use config::Config;
use log::{error, info};
use runinator_api::AsyncApiClient;
use runinator_comm::discovery::{WebServiceDiscovery, start_web_service_listener};
use runinator_models::workflows::WorkflowBundle;
use tokio::time::{self, Duration};

type DynError = Box<dyn std::error::Error + Send + Sync>;
type ApiClient = AsyncApiClient<GossipServiceLocator>;

#[derive(Clone)]
struct GossipServiceLocator {
    inner: WebServiceDiscovery,
}

impl GossipServiceLocator {
    async fn from_config(config: &Config) -> Result<Self, std::io::Error> {
        let inner =
            start_web_service_listener(config.gossip_bind.as_str(), config.gossip_port).await?;
        Ok(Self { inner })
    }
}

impl std::ops::Deref for GossipServiceLocator {
    type Target = WebServiceDiscovery;

    fn deref(&self) -> &Self::Target {
        &self.inner
    }
}

impl AsRef<WebServiceDiscovery> for GossipServiceLocator {
    fn as_ref(&self) -> &WebServiceDiscovery {
        &self.inner
    }
}

#[async_trait]
impl runinator_api::ServiceLocator for GossipServiceLocator {
    type Error = Infallible;

    async fn wait_for_service_url(&self) -> Result<String, Self::Error> {
        Ok(self.inner.wait_for_service_url().await)
    }
}

#[tokio::main]
async fn main() -> Result<(), DynError> {
    env_logger::init();
    let config = Config::parse();

    info!("Starting Runinator Importer");
    let discovery = GossipServiceLocator::from_config(&config).await?;
    let http_client = reqwest::Client::builder()
        .timeout(Duration::from_secs(30))
        .build()?;
    let api = ApiClient::with_client(discovery.clone(), http_client);

    if config.once {
        let mut last_modified = None;
        sync_workflows_if_changed(&config, &api, &mut last_modified).await?;
        return Ok(());
    }

    let mut interval = time::interval(Duration::from_secs(config.poll_interval_seconds.max(1)));
    let mut last_modified: Option<SystemTime> = None;

    loop {
        tokio::select! {
            _ = interval.tick() => {
                if let Err(err) = sync_workflows_if_changed(&config, &api, &mut last_modified).await {
                    error!("Failed to synchronize workflows: {}", err);
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

async fn sync_workflows_if_changed(
    config: &Config,
    api: &ApiClient,
    last_modified: &mut Option<SystemTime>,
) -> Result<(), DynError> {
    let path = Path::new(&config.workflows_file);
    let metadata = tokio::fs::metadata(path).await?;
    let modified = metadata.modified()?;

    let should_sync = last_modified.map_or(true, |prev| modified > prev);
    if !should_sync {
        return Ok(());
    }

    let seed = load_import_file(path).await?;
    info!(
        "Seeding {} workflow(s) from {}",
        seed.workflows.len(),
        path.display()
    );
    for workflow in seed.workflows {
        let _ = api
            .upsert_workflow(&workflow)
            .await
            .map_err(|err| -> DynError { Box::new(err) })?;
    }
    info!(
        "Seeding {} workflow trigger(s) from {}",
        seed.triggers.len(),
        path.display()
    );
    for trigger in seed.triggers {
        let _ = api
            .upsert_workflow_trigger(&trigger)
            .await
            .map_err(|err| -> DynError { Box::new(err) })?;
    }

    *last_modified = Some(modified);
    Ok(())
}

async fn load_import_file(path: &Path) -> Result<WorkflowBundle, DynError> {
    let data = tokio::fs::read_to_string(path).await?;
    Ok(serde_json::from_str(&data)?)
}
