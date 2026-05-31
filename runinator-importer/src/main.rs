mod config;
#[cfg(test)]
mod tests;

use std::{convert::Infallible, io, path::Path, time::SystemTime};

use async_trait::async_trait;
use clap::Parser;
use config::Config;
use log::{error, info, warn};
use runinator_api::{AsyncApiClient, ServiceLocator, StaticLocator};
use runinator_comm::discovery::{WebServiceDiscovery, start_web_service_listener};
use runinator_models::value::Value;
use runinator_models::{
    bundles::{ProviderBundle, SecretBundle},
    types::RuninatorType,
    workflows::{WorkflowBundle, WorkflowDefinition, WorkflowGraph, WorkflowTrigger},
};
use runinator_plugin::provider::Provider;
use runinator_provider_ai::AiCommandProvider;
use runinator_provider_approval::ApprovalProvider;
use runinator_provider_aws::AwsProvider;
use runinator_provider_console::ConsoleProvider;
use runinator_provider_email::EmailProvider;
use runinator_provider_git::GitProvider;
use runinator_provider_github::GitHubProvider;
use runinator_provider_jira::JiraProvider;
use runinator_provider_slack::SlackProvider;
use runinator_provider_sql::SqlProvider;
use runinator_utilities::app_data;
use tokio::time::{self, Duration};

type DynError = Box<dyn std::error::Error + Send + Sync>;
type ApiClient = AsyncApiClient<ImporterServiceLocator>;

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
trait SecretBundleImporter: Send + Sync {
    async fn import_secret_bundle(
        &self,
        bundle: &SecretBundle,
    ) -> runinator_api::Result<SecretBundle>;
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

#[async_trait]
impl<L> SecretBundleImporter for AsyncApiClient<L>
where
    L: ServiceLocator,
{
    async fn import_secret_bundle(
        &self,
        bundle: &SecretBundle,
    ) -> runinator_api::Result<SecretBundle> {
        AsyncApiClient::import_secret_bundle(self, bundle).await
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

#[derive(Clone)]
enum ImporterServiceLocator {
    Static(StaticLocator),
    Gossip(GossipServiceLocator),
}

#[async_trait]
impl ServiceLocator for ImporterServiceLocator {
    type Error = Infallible;

    async fn wait_for_service_url(&self) -> Result<String, Self::Error> {
        match self {
            Self::Static(locator) => locator.wait_for_service_url().await,
            Self::Gossip(locator) => locator.wait_for_service_url().await,
        }
    }
}

#[tokio::main]
async fn main() -> Result<(), DynError> {
    env_logger::init();
    let config = Config::parse();

    info!("Starting Runinator Importer");
    let locator = build_service_locator(&config).await?;
    let http_client = reqwest::Client::builder()
        .timeout(Duration::from_secs(30))
        .build()?;
    let api = ApiClient::with_client(locator, http_client);

    publish_provider_bundle(&api).await;
    publish_secret_bundle(&config, &api).await;

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

async fn build_service_locator(config: &Config) -> Result<ImporterServiceLocator, std::io::Error> {
    if let Some(base_url) = non_empty_api_base_url(config) {
        info!("Using configured Runinator web service URL: {base_url}");
        return Ok(ImporterServiceLocator::Static(StaticLocator::new(
            base_url.to_string(),
        )));
    }

    info!("Discovering Runinator web service via gossip");
    Ok(ImporterServiceLocator::Gossip(
        GossipServiceLocator::from_config(config).await?,
    ))
}

fn non_empty_api_base_url(config: &Config) -> Option<&str> {
    config
        .api_base_url
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
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
            EmailProvider {}.metadata(),
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

async fn publish_secret_bundle(config: &Config, api: &impl SecretBundleImporter) {
    let path = secret_bundle_path(config);
    if !path.exists() {
        info!("No secret bundle found at {}", path.display());
        return;
    }

    match load_secret_bundle(&path).await {
        Ok(bundle) => {
            let count = bundle.secrets.len();
            match api.import_secret_bundle(&bundle).await {
                Ok(imported) => info!(
                    "Imported {} secret(s) from {}",
                    imported.secrets.len(),
                    path.display()
                ),
                Err(err) => warn!("Failed to import secret bundle ({count} secret(s)): {err}"),
            }
        }
        Err(err) => warn!(
            "Failed to load secret bundle at {}: {}",
            path.display(),
            err
        ),
    }
}

fn secret_bundle_path(config: &Config) -> std::path::PathBuf {
    config
        .secrets_file
        .as_ref()
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| {
            app_data::default_secret_bundle_path()
                .unwrap_or_else(|_| std::path::PathBuf::from(".runinator/secrets.json"))
        })
}

async fn load_secret_bundle(path: &Path) -> Result<SecretBundle, DynError> {
    let data = tokio::fs::read_to_string(path).await?;
    Ok(serde_json::from_str(&data)?)
}

async fn sync_workflows_if_changed(
    config: &Config,
    api: &impl WorkflowBundleImporter,
    last_modified: &mut Option<SystemTime>,
) -> Result<(), DynError> {
    let path = workflow_bundle_path(config);
    let metadata = tokio::fs::metadata(&path)
        .await
        .map_err(|err| path_io_error("inspect workflow bundle at", &path, err))?;
    let modified = metadata.modified()?;

    let should_sync = last_modified.is_none_or(|prev| modified > prev);
    if !should_sync {
        return Ok(());
    }

    let bundle = load_import_file(&path).await?;
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

fn workflow_bundle_path(config: &Config) -> std::path::PathBuf {
    config
        .workflows_file
        .as_ref()
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| {
            app_data::app_data_path("workflows/workflow-pack.json").unwrap_or_else(|_| {
                std::path::PathBuf::from(".runinator/workflows/workflow-pack.json")
            })
        })
}

async fn load_import_file(path: &Path) -> Result<WorkflowBundle, DynError> {
    // a directory is treated as a pack: every *.wdl inside becomes one workflow.
    if path.is_dir() {
        return load_wdl_directory(path).await;
    }

    let extension = path.extension().and_then(|ext| ext.to_str());

    // a .wdlp manifest lists the .wdl files (and triggers) that make up a multi-workflow pack.
    if extension == Some("wdlp") {
        return load_wdl_pack_manifest(path).await;
    }

    let data = tokio::fs::read_to_string(path)
        .await
        .map_err(|err| path_io_error("read workflow bundle at", path, err))?;

    // a .wdl file is compiled into a single-workflow bundle.
    if extension == Some("wdl") {
        let definition = compile_wdl(path, &data, 1)?;
        return Ok(WorkflowBundle {
            workflows: vec![definition],
            triggers: Vec::new(),
        });
    }

    let raw: Value = serde_json::from_str(&data)?;

    if raw.get("item_type").and_then(Value::as_str) == Some("workflow_pack") {
        return unwrap_workflow_pack(raw);
    }

    Ok(serde_json::from_value(raw.into())?)
}

// format and compile one .wdl source into a definition.
// imported workflows are enabled so a pack is live as soon as it lands.
fn compile_wdl(
    path: &Path,
    data: &str,
    default_version: i64,
) -> Result<WorkflowDefinition, DynError> {
    let options = runinator_wdl::CompileOptions {
        enabled: true,
        default_version,
    };
    let formatted = runinator_wdl::format_str(data).map_err(|err| -> DynError {
        format!(
            "failed to format {} before import:\n{}",
            path.display(),
            err.render(data)
        )
        .into()
    })?;
    runinator_wdl::compile_str(&formatted, &options).map_err(|err| -> DynError {
        format!(
            "failed to compile {}:\n{}",
            path.display(),
            err.render(&formatted)
        )
        .into()
    })
}

// compile every *.wdl in a directory (sorted for deterministic ids) into one bundle.
async fn load_wdl_directory(dir: &Path) -> Result<WorkflowBundle, DynError> {
    let mut wdl_paths = Vec::new();
    let mut entries = tokio::fs::read_dir(dir)
        .await
        .map_err(|err| path_io_error("read workflow directory at", dir, err))?;
    while let Some(entry) = entries
        .next_entry()
        .await
        .map_err(|err| path_io_error("read directory entry in", dir, err))?
    {
        let entry_path = entry.path();
        if entry_path.extension().and_then(|ext| ext.to_str()) == Some("wdl") {
            wdl_paths.push(entry_path);
        }
    }
    wdl_paths.sort();
    if wdl_paths.is_empty() {
        return Err(format!("no .wdl files found in {}", dir.display()).into());
    }

    let mut workflows = Vec::with_capacity(wdl_paths.len());
    for wdl_path in &wdl_paths {
        let data = tokio::fs::read_to_string(wdl_path)
            .await
            .map_err(|err| path_io_error("read workflow at", wdl_path, err))?;
        workflows.push(compile_wdl(wdl_path, &data, 1)?);
    }
    Ok(WorkflowBundle {
        workflows,
        triggers: Vec::new(),
    })
}

// resolve a .wdlp manifest: compile each referenced .wdl (relative to the manifest) and
// pass through any declared triggers.
async fn load_wdl_pack_manifest(path: &Path) -> Result<WorkflowBundle, DynError> {
    let data = tokio::fs::read_to_string(path)
        .await
        .map_err(|err| path_io_error("read workflow pack manifest at", path, err))?;
    let manifest: Value = serde_json::from_str(&data)?;
    let base_dir = path.parent().unwrap_or_else(|| Path::new("."));

    let version = manifest
        .get("version")
        .and_then(|v| {
            v.as_str()
                .and_then(|s| s.parse::<i64>().ok())
                .or_else(|| v.as_i64())
        })
        .unwrap_or(1);

    let entries = manifest
        .get("workflows")
        .and_then(Value::as_array)
        .ok_or_else(|| -> DynError { "wdl pack manifest missing 'workflows' array".into() })?;

    let mut workflows = Vec::with_capacity(entries.len());
    for entry in entries {
        let rel = entry
            .as_str()
            .or_else(|| entry.get("path").and_then(Value::as_str))
            .ok_or_else(|| -> DynError {
                "each manifest workflow entry must be a path string or have a 'path'".into()
            })?;
        let wdl_path = base_dir.join(rel);
        let source = tokio::fs::read_to_string(&wdl_path)
            .await
            .map_err(|err| path_io_error("read manifest workflow at", &wdl_path, err))?;
        workflows.push(compile_wdl(&wdl_path, &source, version)?);
    }

    let triggers = match manifest.get("triggers").cloned() {
        Some(value) if !value.is_null() => {
            serde_json::from_value::<Vec<WorkflowTrigger>>(value.into())?
        }
        _ => Vec::new(),
    };

    Ok(WorkflowBundle {
        workflows,
        triggers,
    })
}

fn path_io_error(action: &str, path: &Path, err: io::Error) -> io::Error {
    io::Error::new(
        err.kind(),
        format!("failed to {action} {}: {err}", path.display()),
    )
}

// remove a timestamp field from a pack body and parse it as a UTC datetime.
fn take_pack_timestamp(body: &mut Value, key: &str) -> Option<chrono::DateTime<chrono::Utc>> {
    body.as_object_mut()
        .and_then(|object| object.remove(key))
        .and_then(|value| serde_json::from_value(value.into()).ok())
}

fn unwrap_workflow_pack(envelope: Value) -> Result<WorkflowBundle, DynError> {
    let version = envelope
        .get("version")
        .and_then(|v| {
            v.as_str()
                .and_then(|s| s.parse::<i64>().ok())
                .or_else(|| v.as_i64())
        })
        .unwrap_or(1);

    let document = envelope
        .get("document")
        .ok_or_else(|| -> DynError { "workflow pack envelope missing 'document'".into() })?;

    let workflows_map = document
        .get("workflows")
        .and_then(Value::as_object)
        .ok_or_else(|| -> DynError {
            "workflow pack envelope missing document.workflows map".into()
        })?;

    let mut workflows = Vec::with_capacity(workflows_map.len());
    for (name, body) in workflows_map {
        let mut body = body.clone();
        let input_type_value = body
            .as_object_mut()
            .and_then(|o| o.remove("input_type").or_else(|| o.remove("input_schema")))
            .unwrap_or(Value::Null);
        let input_type = serde_json::from_value(input_type_value.clone().into())
            .unwrap_or_else(|_| RuninatorType::from_json_schema(&input_type_value));
        // lift timestamps out of the definition body so import reconciliation can
        // compare them; their absence keeps the existing copy untouched on import.
        let created_at = take_pack_timestamp(&mut body, "created_at");
        let updated_at = take_pack_timestamp(&mut body, "updated_at");
        workflows.push(WorkflowDefinition {
            id: None,
            name: name.clone(),
            version,
            enabled: true,
            input_type,
            definition: WorkflowGraph::from_value(body)
                .map_err(|err| format!("workflow '{name}' definition is invalid: {err}"))?,
            created_at,
            updated_at,
        });
    }

    let triggers = match document.get("triggers").cloned() {
        Some(value) if !value.is_null() => {
            serde_json::from_value::<Vec<WorkflowTrigger>>(value.into())?
        }
        _ => Vec::new(),
    };

    Ok(WorkflowBundle {
        workflows,
        triggers,
    })
}
