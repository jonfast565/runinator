use std::time::Duration;

use reqwest::Url;
use serde_json::Value;

use crate::{
    API_ROOT, GitHubOperation, GitHubTransport, errors::GitHubError, execute_cli, execute_http,
};

#[derive(Clone)]
pub struct AsyncGitHubClient {
    pub(crate) transport: GitHubTransport,
}

impl AsyncGitHubClient {
    pub fn http(token: impl Into<String>, timeout: Duration) -> Result<Self, GitHubError> {
        Self::http_at(API_ROOT, token, timeout)
    }

    pub fn http_at(
        api_root: &str,
        token: impl Into<String>,
        timeout: Duration,
    ) -> Result<Self, GitHubError> {
        let client = reqwest::Client::builder()
            .timeout(timeout)
            .user_agent("runinator")
            .build()
            .map_err(GitHubError::request)?;
        let api_root = Url::parse(api_root).map_err(GitHubError::config)?;
        Ok(Self {
            transport: GitHubTransport::Http {
                client,
                token: token.into(),
                api_root,
            },
        })
    }

    pub fn cli(token: Option<String>, timeout: Duration, max_output_bytes: usize) -> Self {
        Self {
            transport: GitHubTransport::Cli {
                token,
                timeout,
                max_output_bytes,
            },
        }
    }

    pub async fn execute(&self, operation: GitHubOperation) -> Result<Value, GitHubError> {
        let spec = operation.request();
        match &self.transport {
            GitHubTransport::Http {
                client,
                token,
                api_root,
            } => execute_http(client, token, api_root, spec).await,
            GitHubTransport::Cli {
                token,
                timeout,
                max_output_bytes,
            } => execute_cli(token.as_deref(), *timeout, *max_output_bytes, spec).await,
        }
    }
}
