use reqwest::{
    blocking::{Client, Response},
    Url,
};
use runinator_models::{
    bundles::{Bundle, ProviderBundle},
    providers::ProviderMetadata,
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
        let url = self.build_url("/providers")?;
        let response = self.client.get(url.clone()).send()?;
        let response = Self::handle_response(url, response)?;
        Ok(response.json::<Vec<ProviderMetadata>>()?)
    }

    /// Register provider/action metadata with the web service.
    pub fn upsert_provider(&self, provider: &ProviderMetadata) -> Result<ProviderMetadata> {
        let url = self.build_url("/providers")?;
        let response = self.client.post(url.clone()).json(provider).send()?;
        let response = Self::handle_response(url, response)?;
        Ok(response.json::<ProviderMetadata>()?)
    }

    pub fn validate_workflow(&self, workflow: &WorkflowDefinition) -> Result<WorkflowDefinition> {
        let url = self.build_url("/workflows/validate")?;
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

    pub fn export_workflow_bundle(&self, workflow_id: Option<i64>) -> Result<WorkflowBundle> {
        let path = workflow_id
            .map(|id| format!("/workflows/{id}/export"))
            .unwrap_or_else(|| "/workflows/export".into());
        let url = self.build_url(&path)?;
        let response = self.client.get(url.clone()).send()?;
        let response = Self::handle_response(url, response)?;
        Ok(response.json::<WorkflowBundle>()?)
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
