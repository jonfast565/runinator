use reqwest::{
    blocking::{Client, Response},
    Url,
};
use runinator_models::json;
use runinator_models::{
    api_routes::{api_workflow, api_workflow_run_command, API_PROVIDERS, API_WORKFLOWS_VALIDATE},
    bundles::{Bundle, ProviderBundle, SecretBundle},
    providers::ProviderMetadata,
    web::TaskResponse,
    workflows::{WorkflowBundle, WorkflowDefinition},
};

use crate::{
    error::{ApiError, Result},
    locator::BlockingServiceLocator,
};

/// Blocking API client that wraps `reqwest::blocking::Client`.
#[derive(Clone)]
pub struct BlockingApiClient<L> {
    client: Client,
    locator: L,
}

impl<L> BlockingApiClient<L>
where
    L: BlockingServiceLocator,
{
    /// Construct a client with the default `reqwest::blocking::Client` configuration.
    pub fn new(locator: L) -> reqwest::Result<Self> {
        let client = Client::builder().build()?;
        Ok(Self { client, locator })
    }

    /// Construct a client using a preconfigured HTTP client instance.
    pub fn with_client(locator: L, client: Client) -> Self {
        Self { client, locator }
    }

    /// Fetch provider/action metadata for task authoring.
    pub fn fetch_providers(&self) -> Result<Vec<ProviderMetadata>> {
        let url = self.build_url(API_PROVIDERS)?;
        let response = self.client.get(url.clone()).send()?;
        let response = Self::handle_response(url, response)?;
        Ok(response.json::<Vec<ProviderMetadata>>()?)
    }

    /// Register provider/action metadata with the web service.
    pub fn upsert_provider(&self, provider: &ProviderMetadata) -> Result<ProviderMetadata> {
        let url = self.build_url(API_PROVIDERS)?;
        let response = self.client.post(url.clone()).json(provider).send()?;
        let response = Self::handle_response(url, response)?;
        Ok(response.json::<ProviderMetadata>()?)
    }

    pub fn validate_workflow(&self, workflow: &WorkflowDefinition) -> Result<WorkflowDefinition> {
        let url = self.build_url(API_WORKFLOWS_VALIDATE)?;
        let response = self.client.post(url.clone()).json(workflow).send()?;
        let response = Self::handle_response(url, response)?;
        Ok(response.json::<WorkflowDefinition>()?)
    }

    /// POST a typed bundle to its associated import endpoint.
    pub fn import_bundle<B: Bundle>(&self, bundle: &B) -> Result<B> {
        let url = self.build_url(B::RESOURCE)?;
        let response = self.client.post(url.clone()).json(bundle).send()?;
        let response = Self::handle_response(url, response)?;
        Ok(response.json::<B>()?)
    }

    pub fn import_workflow_bundle(&self, bundle: &WorkflowBundle) -> Result<WorkflowBundle> {
        self.import_bundle(bundle)
    }

    pub fn import_provider_bundle(&self, bundle: &ProviderBundle) -> Result<ProviderBundle> {
        self.import_bundle(bundle)
    }

    pub fn import_secret_bundle(&self, bundle: &SecretBundle) -> Result<SecretBundle> {
        self.import_bundle(bundle)
    }

    pub fn export_workflow_bundle(&self, workflow_id: Option<i64>) -> Result<WorkflowBundle> {
        let path = workflow_id
            .map(|id| format!("{}/export", api_workflow(id)))
            .unwrap_or_else(|| runinator_models::api_routes::API_WORKFLOWS_EXPORT.into());
        let url = self.build_url(&path)?;
        let response = self.client.get(url.clone()).send()?;
        let response = Self::handle_response(url, response)?;
        Ok(response.json::<WorkflowBundle>()?)
    }

    pub fn pause_workflow_run(&self, workflow_run_id: i64) -> Result<TaskResponse> {
        self.post_workflow_run_command(workflow_run_id, "pause")
    }

    pub fn resume_workflow_run(&self, workflow_run_id: i64) -> Result<TaskResponse> {
        self.post_workflow_run_command(workflow_run_id, "resume")
    }

    pub fn cancel_workflow_run(&self, workflow_run_id: i64) -> Result<TaskResponse> {
        self.post_workflow_run_command(workflow_run_id, "cancel")
    }

    fn post_workflow_run_command(
        &self,
        workflow_run_id: i64,
        command: &str,
    ) -> Result<TaskResponse> {
        let url = self.build_url(&api_workflow_run_command(workflow_run_id, command))?;
        let response = self.client.post(url.clone()).json(&json!({})).send()?;
        let response = Self::handle_response(url, response)?;
        Ok(response.json::<TaskResponse>()?)
    }

    fn build_url(&self, path: &str) -> Result<Url> {
        let base = self
            .locator
            .wait_for_service_url()
            .map_err(ApiError::discovery)?;
        let base_url = Url::parse(&base).map_err(|source| ApiError::InvalidBaseUrl {
            url: base.clone(),
            source,
        })?;
        let trimmed_path = path.trim_start_matches('/');
        base_url
            .join(trimmed_path)
            .map_err(|source| ApiError::InvalidPath {
                base: base_url.clone(),
                path: trimmed_path.to_string(),
                source,
            })
    }

    fn handle_response(url: Url, response: Response) -> Result<Response> {
        let status = response.status();
        if status.is_success() {
            Ok(response)
        } else {
            let message = response
                .text()
                .unwrap_or_else(|_| "<unable to read body>".into());
            Err(ApiError::Http {
                status,
                url,
                message,
            })
        }
    }
}
