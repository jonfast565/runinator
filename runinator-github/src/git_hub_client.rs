use std::time::Duration;

use serde_json::Value;

use crate::{AsyncGitHubClient, GitHubOperation, errors::GitHubError};

pub struct GitHubClient {
    inner: AsyncGitHubClient,
    runtime: tokio::runtime::Runtime,
}

impl GitHubClient {
    pub fn http(token: impl Into<String>, timeout: Duration) -> Result<Self, GitHubError> {
        Self::from_async(AsyncGitHubClient::http(token, timeout)?)
    }

    pub fn http_at(
        api_root: &str,
        token: impl Into<String>,
        timeout: Duration,
    ) -> Result<Self, GitHubError> {
        Self::from_async(AsyncGitHubClient::http_at(api_root, token, timeout)?)
    }

    pub fn cli(
        token: Option<String>,
        timeout: Duration,
        max_output_bytes: usize,
    ) -> Result<Self, GitHubError> {
        Self::from_async(AsyncGitHubClient::cli(token, timeout, max_output_bytes))
    }

    pub fn execute(&self, operation: GitHubOperation) -> Result<Value, GitHubError> {
        self.runtime.block_on(self.inner.execute(operation))
    }

    fn from_async(inner: AsyncGitHubClient) -> Result<Self, GitHubError> {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .map_err(GitHubError::runtime)?;
        Ok(Self { inner, runtime })
    }
}
