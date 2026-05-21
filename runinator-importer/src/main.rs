mod config;
#[cfg(test)]
mod tests;

use std::{convert::Infallible, path::Path, time::SystemTime};

use async_trait::async_trait;
use clap::Parser;
use config::Config;
use log::{error, info, warn};
use runinator_api::{AsyncApiClient, ServiceLocator};
use runinator_comm::discovery::{WebServiceDiscovery, start_web_service_listener};
use runinator_models::{
    bundles::ProviderBundle,
    workflows::{WorkflowBundle, WorkflowDefinition, WorkflowTrigger},
};
use runinator_plugin::provider::Provider;
use runinator_provider_ai::AiCommandProvider;
use runinator_provider_approval::ApprovalProvider;
use runinator_provider_aws::AwsProvider;
use runinator_provider_console::ConsoleProvider;
use runinator_provider_git::GitProvider;
use runinator_provider_github::GitHubProvider;
use runinator_provider_jira::JiraProvider;
use runinator_provider_slack::SlackProvider;
use runinator_provider_sql::SqlProvider;
use serde_json::Value;
use tokio::time::{self, Duration};

type DynError = Box<dyn std::error::Error + Send + Sync>;
type ApiClient = AsyncApiClient<GossipServiceLocator>;

#[async_trait]
trait WorkflowBundleImporter: Send + Sync {
    async fn import_workflow_bundle(
        &self,
        bundle: &WorkflowBundle,
    ) -> runinator_api::Result<WorkflowBundle>;
}

#[async_trait]
trait ProviderBundleImporter: Send + Sync {
    async fn import_provider_bundle(
        &self,
        bundle: &ProviderBundle,
    ) -> runinator_api::Result<ProviderBundle>;
}

#[async_trait]
impl<L> WorkflowBundleImporter for AsyncApiClient<L>
where
    L: ServiceLocator,
{
    async fn import_workflow_bundle(
        &self,
        bundle: &WorkflowBundle,
    ) -> runinator_api::Result<WorkflowBundle> {
        AsyncApiClient::import_workflow_bundle(self, bundle).await
    }
}

#[async_trait]
impl<L> ProviderBundleImporter for AsyncApiClient<L>
where
    L: ServiceLocator,
{
    async fn import_provider_bundle(
        &self,
        bundle: &ProviderBundle,
    ) -> runinator_api::Result<ProviderBundle> {
        AsyncApiClient::import_provider_bundle(self, bundle).await
    }
}

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

    publish_provider_bundle(&api).await;

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

fn build_provider_bundle() -> ProviderBundle {
    ProviderBundle {
        providers: vec![
            ConsoleProvider {}.metadata(),
            AwsProvider {}.metadata(),
            SqlProvider {}.metadata(),
            JiraProvider {}.metadata(),
            GitHubProvider {}.metadata(),
            SlackProvider {}.metadata(),
            GitProvider {}.metadata(),
            AiCommandProvider {}.metadata(),
            ApprovalProvider {}.metadata(),
        ],
    }
}

async fn publish_provider_bundle(api: &impl ProviderBundleImporter) {
    let bundle = build_provider_bundle();
    let count = bundle.providers.len();
    match api.import_provider_bundle(&bundle).await {
        Ok(imported) => info!(
            "Registered {} provider(s) via /providers/import",
            imported.providers.len()
        ),
        Err(err) => warn!("Failed to register provider bundle ({count} provider(s)): {err}"),
    }
}

async fn sync_workflows_if_changed(
    config: &Config,
    api: &impl WorkflowBundleImporter,
    last_modified: &mut Option<SystemTime>,
) -> Result<(), DynError> {
    let path = Path::new(&config.workflows_file);
    let metadata = tokio::fs::metadata(path).await?;
    let modified = metadata.modified()?;

    let should_sync = last_modified.map_or(true, |prev| modified > prev);
    if !should_sync {
        return Ok(());
    }

    let bundle = load_import_file(path).await?;
    info!(
        "Importing workflow bundle with {} workflow(s) and {} trigger(s) from {}",
        bundle.workflows.len(),
        bundle.triggers.len(),
        path.display()
    );
    let imported = api
        .import_workflow_bundle(&bundle)
        .await
        .map_err(|err| -> DynError { Box::new(err) })?;
    info!(
        "Imported workflow bundle with {} workflow(s) and {} trigger(s) from {}",
        imported.workflows.len(),
        imported.triggers.len(),
        path.display()
    );

    *last_modified = Some(modified);
    Ok(())
}

async fn load_import_file(path: &Path) -> Result<WorkflowBundle, DynError> {
    let data = tokio::fs::read_to_string(path).await?;
    let raw: Value = serde_json::from_str(&data)?;

    if raw.get("item_type").and_then(Value::as_str) == Some("workflow_pack") {
        return unwrap_workflow_pack(raw);
    }

    Ok(serde_json::from_value(raw)?)
}

fn unwrap_workflow_pack(envelope: Value) -> Result<WorkflowBundle, DynError> {
    let version = envelope
        .get("version")
        .and_then(|v| v.as_str().and_then(|s| s.parse::<i64>().ok()).or_else(|| v.as_i64()))
        .unwrap_or(1);

    let document = envelope
        .get("document")
        .ok_or_else(|| -> DynError { "workflow pack envelope missing 'document'".into() })?;

    let workflows_map = document
        .get("workflows")
        .and_then(Value::as_object)
        .ok_or_else(|| -> DynError { "workflow pack envelope missing document.workflows map".into() })?;

    let mut workflows = Vec::with_capacity(workflows_map.len());
    for (name, body) in workflows_map {
        let mut body = body.clone();
        let input_schema = body
            .as_object_mut()
            .and_then(|o| o.remove("input_schema"))
            .unwrap_or(Value::Null);
        workflows.push(WorkflowDefinition {
            id: None,
            name: name.clone(),
            version,
            enabled: true,
            input_schema,
            definition: body,
            created_at: None,
            updated_at: None,
        });
    }

    let triggers = match document.get("triggers").cloned() {
        Some(value) if !value.is_null() => serde_json::from_value::<Vec<WorkflowTrigger>>(value)?,
        _ => Vec::new(),
    };

    Ok(WorkflowBundle {
        workflows,
        triggers,
    })
}
