use reqwest::{
    blocking::{Client, Response},
    Url,
};
use runinator_models::providers::ProviderMetadata;

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
