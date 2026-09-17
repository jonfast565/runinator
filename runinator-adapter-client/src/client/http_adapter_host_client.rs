#[allow(unused_imports)]
use super::*;

/// Clones share a connection pool and circuit; independently constructed clients are isolated.
#[derive(Clone)]
pub struct HttpAdapterHostClient {
    pub(super) base_url: String,
    pub(super) token: Option<String>,
    pub(super) http: std::result::Result<reqwest::Client, String>,
    pub(super) circuit: AdapterCircuit,
}

impl HttpAdapterHostClient {
    pub fn from_env() -> Self {
        Self::new(host_url(), host_token().ok())
    }

    pub fn new(base_url: String, token: Option<String>) -> Self {
        Self {
            base_url: base_url.trim_end_matches('/').to_owned(),
            token,
            http: reqwest::Client::builder()
                .timeout(POLL_TIMEOUT)
                .build()
                .map_err(|error| error.to_string()),
            circuit: AdapterCircuit::from_env(),
        }
    }

    pub(super) fn http(&self) -> Result<&reqwest::Client> {
        self.http
            .as_ref()
            .map_err(|error| AdapterClientError::Configuration(error.clone()))
    }

    pub(super) fn token(&self) -> Result<&str> {
        self.token.as_deref().ok_or_else(|| {
            AdapterClientError::Configuration(
                "RUNINATOR_ADAPTER_HOST_TOKEN is not configured".into(),
            )
        })
    }
    pub(super) async fn get_json<T: DeserializeOwned>(&self, path: &str) -> Result<T> {
        let response = send_with_circuit(
            self.http()?,
            &self.circuit,
            self.http()?
                .get(format!("{}{path}", self.base_url))
                .bearer_auth(self.token()?)
                .timeout(VERIFY_TIMEOUT),
        )
        .await?;
        decode(response).await
    }

    pub(super) async fn post_json<T: DeserializeOwned>(
        &self,
        path: &str,
        body: serde_json::Value,
        timeout: Duration,
    ) -> Result<T> {
        let response = send_with_circuit(
            self.http()?,
            &self.circuit,
            self.http()?
                .post(format!("{}{path}", self.base_url))
                .bearer_auth(self.token()?)
                .timeout(timeout)
                .json(&body),
        )
        .await?;
        decode(response).await
    }
}

#[async_trait]
impl AdapterPoller for HttpAdapterHostClient {
    async fn poll(&self, kind: &str, request: AdapterPollRequest) -> Result<AdapterPollResponse> {
        self.post_json(
            "/poll",
            serde_json::json!({ "kind": kind, "request": request }),
            POLL_TIMEOUT,
        )
        .await
    }
}

#[async_trait]
impl AdapterVerifier for HttpAdapterHostClient {
    async fn verify_normalize(
        &self,
        kind: &str,
        request: AdapterRequest,
    ) -> Result<AdapterResponse> {
        self.post_json(
            "/verify-normalize",
            serde_json::json!({ "kind": kind, "request": request }),
            VERIFY_TIMEOUT,
        )
        .await
    }
}

#[async_trait]
impl AdapterValidator for HttpAdapterHostClient {
    async fn validate(
        &self,
        kind: &str,
        request: AdapterValidationRequest,
    ) -> Result<AdapterValidationResponse> {
        self.post_json(
            "/validate",
            serde_json::json!({ "kind": kind, "request": request }),
            VERIFY_TIMEOUT,
        )
        .await
    }
}

#[async_trait]
impl AdapterHostAdmin for HttpAdapterHostClient {
    fn host_url(&self) -> &str {
        &self.base_url
    }
    fn token_configured(&self) -> bool {
        self.token.is_some()
    }
    async fn kinds(&self) -> Result<Vec<AdapterKindCatalogEntry>> {
        self.get_json("/kinds").await
    }
    async fn health(&self) -> Result<serde_json::Value> {
        self.get_json("/health").await
    }
    async fn reload(&self) -> Result<serde_json::Value> {
        self.post_json("/reload", serde_json::json!({}), VERIFY_TIMEOUT)
            .await
    }
}
