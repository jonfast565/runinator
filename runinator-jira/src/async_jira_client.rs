use std::time::Duration;

use reqwest::Url;
use serde_json::{Value, json};

use crate::{
    JiraCredentials, JiraOperation, errors::JiraError, request_spec::RequestSpec, validate_base_url,
};

#[derive(Clone)]
pub struct AsyncJiraClient {
    base_url: Url,
    credentials: JiraCredentials,
    client: reqwest::Client,
}

impl AsyncJiraClient {
    pub fn new(
        base_url: &str,
        credentials: JiraCredentials,
        timeout: Duration,
    ) -> Result<Self, JiraError> {
        let base_url = validate_base_url(base_url)?;
        let client = reqwest::Client::builder()
            .timeout(timeout)
            .build()
            .map_err(JiraError::request)?;
        Ok(Self {
            base_url,
            credentials,
            client,
        })
    }

    pub async fn execute(&self, operation: JiraOperation) -> Result<Value, JiraError> {
        let spec = operation.request(&self.base_url)?;
        let (status, text, retry_after) = self.send(spec).await?;
        if !(200..300).contains(&status) {
            return Err(JiraError::status(status, text, retry_after));
        }
        if text.trim().is_empty() {
            return Ok(json!({ "status": status }));
        }
        serde_json::from_str(&text).map_err(JiraError::json)
    }

    pub async fn download(&self, url: String) -> Result<Vec<u8>, JiraError> {
        let spec = JiraOperation::Download { url }.request(&self.base_url)?;
        let request = self
            .client
            .request(spec.method, spec.url)
            .basic_auth(&self.credentials.email, Some(&self.credentials.token));
        let response = request.send().await.map_err(JiraError::request)?;
        let retry_after = response
            .headers()
            .get("retry-after")
            .and_then(|value| value.to_str().ok())
            .and_then(|value| value.parse().ok());
        let status = response.status().as_u16();
        if !(200..300).contains(&status) {
            let body = response.text().await.unwrap_or_default();
            return Err(JiraError::status(status, body, retry_after));
        }
        Ok(response.bytes().await.map_err(JiraError::request)?.to_vec())
    }

    async fn send(&self, spec: RequestSpec) -> Result<(u16, String, Option<u64>), JiraError> {
        let mut request = self
            .client
            .request(spec.method, spec.url)
            .basic_auth(&self.credentials.email, Some(&self.credentials.token))
            .query(&spec.query);
        if let Some(body) = spec.body {
            request = request.json(&body);
        }
        let response = request.send().await.map_err(JiraError::request)?;
        let retry_after = response
            .headers()
            .get("retry-after")
            .and_then(|value| value.to_str().ok())
            .and_then(|value| value.parse().ok());
        let status = response.status().as_u16();
        let text = response.text().await.map_err(JiraError::request)?;
        Ok((status, text, retry_after))
    }
}
