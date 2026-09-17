//! Profile-backed, non-interactive GitHub CLI provider.

pub mod errors;

use std::{
    collections::BTreeMap,
    process::{Command, Stdio},
    sync::Arc,
    time::Duration,
};

use runinator_models::{
    errors::SendableError,
    json,
    orchestration::DeliverySemantics,
    providers::{
        ActionAuthenticationAlternative, ActionAuthenticationMetadata, ActionMetadata,
        CredentialInjection, ExecutionProfileSupport, ParameterMetadata, ProviderMetadata,
        ProviderRuntimeMetadata, ResultMetadata, RuninatorType,
    },
    runs::{ProviderExecutionRequest, TaskExecutionResult},
};
use runinator_plugin::{
    cancel::CancellationToken,
    provider::{Provider, ProviderEventSink},
};
use runinator_provider_support::{
    process_runner::{NativeProcessRunner, ProcessFailure, ProcessRequest, ProcessRunner},
    resolve_working_dir,
};
use serde::Deserialize;
use serde_json::Value;

const MAX_OUTPUT_BYTES: usize = 1024 * 1024;
const ALLOWED_COMMANDS: &[&str] = &[
    "pr", "issue", "run", "workflow", "release", "repo", "search",
];

#[allow(non_upper_case_globals)]
pub const GitHubCliProvider: GitHubCliProvider = GitHubCliProvider {
    runner: NativeProcessRunner,
};

fn default_method() -> String {
    "GET".into()
}

fn with_github_cli_authentication(mut action: ActionMetadata) -> ActionMetadata {
    action.parameters.push(
        ParameterMetadata::optional("token", RuninatorType::String)
            .secret()
            .inject(CredentialInjection::Environment {
                name: "GH_TOKEN".into(),
                template: "${secret}".into(),
            }),
    );
    action.authentication = Some(ActionAuthenticationMetadata::required(vec![
        ActionAuthenticationAlternative::secrets(["token"]),
        ActionAuthenticationAlternative::ExecutionProfile,
    ]));
    action
}

fn apply_workspace_dir(
    command: &mut Command,
    workspace_path: Option<&str>,
) -> Result<(), SendableError> {
    if let Some(directory) = resolve_working_dir(workspace_path, None)? {
        command.current_dir(directory);
    }
    Ok(())
}

fn sanitize_stderr(stderr: &str) -> String {
    let trimmed = stderr.trim();
    if trimmed.len() <= 4_096 {
        trimmed.to_string()
    } else {
        format!("{}…", trimmed.chars().take(4_095).collect::<String>())
    }
}

/// Resolve the token held by an already-materialized GitHub CLI profile. This is intentionally
/// process-local: callers can reuse existing typed HTTP implementations without persisting or
/// returning the credential.
pub fn resolve_auth_token(
    profile: &runinator_models::execution_profiles::MaterializedExecutionProfile,
    timeout: Duration,
) -> Result<String, SendableError> {
    let mut command = Command::new("gh");
    command
        .args(["auth", "token"])
        .env_remove("GH_TOKEN")
        .env_remove("GITHUB_TOKEN")
        .env("GH_PROMPT_DISABLED", "1")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    if let Some(home) = &profile.home {
        command.env("HOME", home);
    }
    command.envs(&profile.environment);
    let cancellation = CancellationToken::new();
    let result = NativeProcessRunner
        .run(ProcessRequest {
            command: &mut command,
            input: None,
            timeout,
            cancellation: &cancellation,
            sink: None,
        })
        .map_err(|error| process_failure(error, timeout))?;
    let status = result.status;
    let output = result.output;
    if !status.success() {
        return Err(errors::COMMAND_FAILED.error(sanitize_stderr(&output.stderr)));
    }
    let token = output.stdout.trim();
    if token.is_empty() || token.len() > 16 * 1024 {
        return Err(errors::COMMAND_FAILED.error("gh auth token returned no usable credential"));
    }
    Ok(token.to_string())
}

fn process_failure(error: ProcessFailure, timeout: Duration) -> SendableError {
    match error {
        ProcessFailure::Spawn(error) => errors::COMMAND_START.error(error),
        ProcessFailure::Io(error) => errors::COMMAND_FAILED.error(error),
        ProcessFailure::Canceled => errors::COMMAND_CANCELED.bare(),
        ProcessFailure::TimedOut => {
            errors::COMMAND_TIMEOUT.error(format!("timed out after {} seconds", timeout.as_secs()))
        }
    }
}

#[cfg(test)]
#[path = "tests.rs"]
mod tests;

#[cfg(test)]
mod runner_tests;

mod git_hub_cli_provider;
pub use git_hub_cli_provider::GitHubCliProvider;

mod api_params;
use api_params::ApiParams;

mod graphql_params;
use graphql_params::GraphqlParams;

mod run_params;
use run_params::RunParams;
