//! Profile-backed, non-interactive GitHub CLI provider.

pub mod errors;

use std::{
    collections::BTreeMap,
    io::Write,
    process::{Command, Stdio},
    sync::Arc,
    thread,
    time::{Duration, Instant},
};

use runinator_models::{
    errors::SendableError,
    json,
    orchestration::DeliverySemantics,
    providers::{
        ActionMetadata, ExecutionProfileSupport, ParameterMetadata, ProviderMetadata,
        ProviderRuntimeMetadata, ResultMetadata, RuninatorType,
    },
    runs::{ProviderExecutionRequest, TaskExecutionResult},
};
use runinator_plugin::{
    cancel::CancellationToken,
    provider::{Provider, ProviderEventSink},
};
use runinator_provider_support::process::ProcessOutputPump;
use serde::Deserialize;
use serde_json::Value;

const MAX_OUTPUT_BYTES: usize = 1024 * 1024;
const ALLOWED_COMMANDS: &[&str] = &[
    "pr", "issue", "run", "workflow", "release", "repo", "search",
];

#[derive(Clone)]
pub struct GitHubCliProvider;

#[derive(Debug, Deserialize)]
struct ApiParams {
    endpoint: String,
    #[serde(default = "default_method")]
    method: String,
    #[serde(default)]
    body: Option<Value>,
    #[serde(default)]
    headers: BTreeMap<String, String>,
    #[serde(default)]
    hostname: Option<String>,
    #[serde(default)]
    paginate: bool,
}

#[derive(Debug, Deserialize)]
struct GraphqlParams {
    query: String,
    #[serde(default)]
    variables: BTreeMap<String, Value>,
    #[serde(default)]
    hostname: Option<String>,
}

#[derive(Debug, Deserialize)]
struct RunParams {
    args: Vec<String>,
}

fn default_method() -> String {
    "GET".into()
}

impl Provider for GitHubCliProvider {
    fn name(&self) -> String {
        "github_cli".into()
    }

    fn metadata(&self) -> ProviderMetadata {
        ProviderMetadata {
            name: self.name(),
            actions: vec![
                ActionMetadata::new("api", "Call a GitHub REST endpoint through gh")
                    .with_parameters(vec![
                        ParameterMetadata::required("endpoint", RuninatorType::String),
                        ParameterMetadata::optional("method", RuninatorType::String)
                            .with_default(json!("GET")),
                        ParameterMetadata::optional("body", RuninatorType::Any),
                        ParameterMetadata::optional(
                            "headers",
                            RuninatorType::map(RuninatorType::String),
                        ),
                        ParameterMetadata::optional("hostname", RuninatorType::String),
                        ParameterMetadata::optional("paginate", RuninatorType::Boolean)
                            .with_default(json!(false)),
                    ])
                    .with_results(vec![ResultMetadata::new("response", RuninatorType::Any)])
                    .with_delivery_semantics(DeliverySemantics::AtLeastOnce),
                ActionMetadata::new("graphql", "Call the GitHub GraphQL API through gh")
                    .with_parameters(vec![
                        ParameterMetadata::required("query", RuninatorType::String),
                        ParameterMetadata::optional(
                            "variables",
                            RuninatorType::map(RuninatorType::Any),
                        ),
                        ParameterMetadata::optional("hostname", RuninatorType::String),
                    ])
                    .with_results(vec![ResultMetadata::new("response", RuninatorType::Any)])
                    .with_delivery_semantics(DeliverySemantics::AtLeastOnce),
                ActionMetadata::new("run", "Run an allowlisted GitHub CLI command")
                    .with_parameters(vec![ParameterMetadata::required(
                        "args",
                        RuninatorType::array(RuninatorType::String),
                    )])
                    .with_results(vec![
                        ResultMetadata::new("stdout", RuninatorType::String),
                        ResultMetadata::new("stderr", RuninatorType::String),
                    ]),
            ],
            metadata: ProviderRuntimeMetadata {
                credential_scopes: vec!["github".into()],
                contract: None,
                execution_profile: ExecutionProfileSupport::Subprocess,
            },
        }
    }

    fn execute_service(
        &self,
        request: ProviderExecutionRequest,
        _sink: Option<Arc<dyn ProviderEventSink>>,
        token: CancellationToken,
    ) -> Result<TaskExecutionResult, SendableError> {
        let profile = request
            .execution_profile
            .as_ref()
            .ok_or_else(|| errors::PROFILE_REQUIRED.bare())?;
        let mut stdin = None;
        let args = match request.action_function.as_str() {
            "api" => {
                let params: ApiParams =
                    runinator_provider_support::parse_params(&request, &errors::INVALID_PARAMS)?;
                let method = params.method.trim().to_ascii_uppercase();
                if !matches!(method.as_str(), "GET" | "POST" | "PUT" | "PATCH" | "DELETE") {
                    return Err(
                        errors::INVALID_PARAMS.error(format!("unsupported HTTP method '{method}'"))
                    );
                }
                let mut args = vec!["api".into(), params.endpoint, "--method".into(), method];
                for (name, value) in params.headers {
                    args.extend(["--header".into(), format!("{name}: {value}")]);
                }
                if let Some(hostname) = params.hostname.filter(|value| !value.trim().is_empty()) {
                    args.extend(["--hostname".into(), hostname]);
                }
                if params.paginate {
                    args.extend(["--paginate".into(), "--slurp".into()]);
                }
                if let Some(body) = params.body {
                    args.extend(["--input".into(), "-".into()]);
                    stdin = Some(serde_json::to_vec(&body)?);
                }
                args
            }
            "graphql" => {
                let params: GraphqlParams =
                    runinator_provider_support::parse_params(&request, &errors::INVALID_PARAMS)?;
                let mut body = serde_json::Map::new();
                body.insert("query".into(), Value::String(params.query));
                body.insert("variables".into(), serde_json::to_value(params.variables)?);
                stdin = Some(serde_json::to_vec(&Value::Object(body))?);
                let mut args = vec!["api".into(), "graphql".into(), "--input".into(), "-".into()];
                if let Some(hostname) = params.hostname.filter(|value| !value.trim().is_empty()) {
                    args.extend(["--hostname".into(), hostname]);
                }
                args
            }
            "run" => {
                let params: RunParams =
                    runinator_provider_support::parse_params(&request, &errors::INVALID_PARAMS)?;
                let Some(command) = params.args.first().map(String::as_str) else {
                    return Err(errors::INVALID_PARAMS.error("args must not be empty"));
                };
                if !ALLOWED_COMMANDS.contains(&command) {
                    return Err(errors::COMMAND_REJECTED
                        .error(format!("command family '{command}' is not allowed")));
                }
                params.args
            }
            other => return Err(errors::UNSUPPORTED_ACTION.error(other)),
        };

        let mut command = Command::new("gh");
        command
            .args(&args)
            .env_remove("GH_TOKEN")
            .env_remove("GITHUB_TOKEN")
            .env("GH_PROMPT_DISABLED", "1")
            .env("GH_PAGER", "cat")
            .env("NO_COLOR", "1")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        if let Some(home) = &profile.home {
            command.env("HOME", home);
        }
        command.envs(&profile.environment);
        let mut child = command
            .spawn()
            .map_err(|error| errors::COMMAND_START.error(error))?;
        let output = ProcessOutputPump::start(&mut child, None)
            .map_err(|error| errors::COMMAND_START.error(error))?;
        if let Some(bytes) = stdin
            && let Some(mut input) = child.stdin.take()
        {
            input.write_all(&bytes)?;
        }
        drop(child.stdin.take());
        let started = Instant::now();
        let timeout = Duration::from_secs(request.timeout_secs.max(1) as u64);
        let status = loop {
            if token.is_cancelled() {
                let _ = child.kill();
                let _ = child.wait();
                return Err(errors::COMMAND_CANCELED.bare());
            }
            if started.elapsed() >= timeout {
                let _ = child.kill();
                let _ = child.wait();
                return Err(errors::COMMAND_TIMEOUT
                    .error(format!("timed out after {} seconds", timeout.as_secs())));
            }
            match child.try_wait() {
                Ok(Some(status)) => break status,
                Ok(None) => thread::sleep(Duration::from_millis(100)),
                Err(error) => return Err(errors::COMMAND_FAILED.error(error)),
            }
        };
        let output = output.finish();
        if output.stdout.len().saturating_add(output.stderr.len()) > MAX_OUTPUT_BYTES {
            return Err(
                errors::OUTPUT_TOO_LARGE.error(format!("maximum is {MAX_OUTPUT_BYTES} bytes"))
            );
        }
        if !status.success() {
            return Err(errors::COMMAND_FAILED.error(sanitize_stderr(&output.stderr)));
        }
        let result = if request.action_function == "run" {
            json!({ "stdout": output.stdout, "stderr": output.stderr })
        } else if output.stdout.trim().is_empty() {
            json!({ "response": null })
        } else {
            let value: Value = serde_json::from_str(&output.stdout)
                .map_err(|error| errors::INVALID_JSON.error(error))?;
            json!({ "response": value })
        };
        Ok(TaskExecutionResult {
            message: Some(format!("github cli {} completed", request.action_function)),
            output_json: Some(result),
            chunks: Vec::new(),
            artifacts: Vec::new(),
        })
    }
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
    let mut child = command
        .spawn()
        .map_err(|error| errors::COMMAND_START.error(error))?;
    let output = ProcessOutputPump::start(&mut child, None)
        .map_err(|error| errors::COMMAND_START.error(error))?;
    let started = Instant::now();
    let status = loop {
        if started.elapsed() >= timeout {
            let _ = child.kill();
            let _ = child.wait();
            return Err(errors::COMMAND_TIMEOUT.error(format!(
                "gh auth token timed out after {} seconds",
                timeout.as_secs()
            )));
        }
        match child.try_wait() {
            Ok(Some(status)) => break status,
            Ok(None) => thread::sleep(Duration::from_millis(100)),
            Err(error) => return Err(errors::COMMAND_FAILED.error(error)),
        }
    };
    let output = output.finish();
    if !status.success() {
        return Err(errors::COMMAND_FAILED.error(sanitize_stderr(&output.stderr)));
    }
    let token = output.stdout.trim();
    if token.is_empty() || token.len() > 16 * 1024 {
        return Err(errors::COMMAND_FAILED.error("gh auth token returned no usable credential"));
    }
    Ok(token.to_string())
}

#[cfg(test)]
#[path = "tests.rs"]
mod tests;
