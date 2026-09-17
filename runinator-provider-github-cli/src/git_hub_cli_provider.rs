#[allow(unused_imports)]
use super::*;

#[derive(Clone)]
pub struct GitHubCliProvider<R = NativeProcessRunner> {
    pub(super) runner: R,
}

impl<R: ProcessRunner> GitHubCliProvider<R> {
    pub fn with_runner(runner: R) -> Self {
        Self { runner }
    }
}

impl<R: ProcessRunner + Clone + 'static> Provider for GitHubCliProvider<R> {
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
            ]
            .into_iter()
            .map(with_github_cli_authentication)
            .collect(),
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
        apply_workspace_dir(&mut command, request.workspace_path.as_deref())?;
        if let Some(profile) = &request.execution_profile {
            if let Some(home) = &profile.home {
                command.env("HOME", home);
            }
            command.envs(&profile.environment);
        }
        runinator_provider_support::apply_command_credentials(&mut command, &request);
        let timeout = Duration::from_secs(request.timeout_secs.max(1) as u64);
        let result = self
            .runner
            .run(ProcessRequest {
                command: &mut command,
                input: stdin,
                timeout,
                cancellation: &token,
                sink: None,
            })
            .map_err(|error| process_failure(error, timeout))?;
        let status = result.status;
        let output = result.output;
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
