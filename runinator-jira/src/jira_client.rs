use std::time::Duration;

use serde_json::Value;

use crate::{AsyncJiraClient, JiraCredentials, JiraOperation, errors::JiraError};

pub struct JiraClient {
    inner: AsyncJiraClient,
    runtime: tokio::runtime::Runtime,
}

impl JiraClient {
    pub fn new(
        base_url: &str,
        credentials: JiraCredentials,
        timeout: Duration,
    ) -> Result<Self, JiraError> {
        let inner = AsyncJiraClient::new(base_url, credentials, timeout)?;
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .map_err(JiraError::runtime)?;
        Ok(Self { inner, runtime })
    }

    pub fn execute(&self, operation: JiraOperation) -> Result<Value, JiraError> {
        self.runtime.block_on(self.inner.execute(operation))
    }

    pub fn download(&self, url: String) -> Result<Vec<u8>, JiraError> {
        self.runtime.block_on(self.inner.download(url))
    }
}
