use std::collections::HashMap;
use std::io::{BufRead, BufReader, Write};
use std::process::{Child, ChildStdout, Command, Stdio};
use std::sync::Arc;
use std::thread::{self, JoinHandle};

use runinator_models::{
    errors::{RuntimeError, SendableError},
    providers::{
        ActionMetadata, ParameterMetadata, ParameterValueType, ProviderMetadata,
        ProviderRuntimeMetadata, ResultMetadata,
    },
    runs::{ProviderExecutionEvent, ProviderExecutionRequest, TaskExecutionResult},
};
use runinator_plugin::provider::{Provider, ProviderEventSink};
use serde::{Deserialize, de::DeserializeOwned};
use serde_json::{Value, json};

#[derive(Deserialize)]
struct AiCommandParams {
    command: String,
    input: Option<Value>,
}

#[derive(Deserialize)]
struct ClaudeCodeParams {
    #[serde(default = "default_binary")]
    binary: String,
    #[serde(default = "default_model")]
    model: String,
    prompt: String,
    #[serde(default)]
    working_dir: Option<String>,
    #[serde(default)]
    allowed_tools: Option<String>,
    #[serde(default = "default_output_format")]
    output_format: String,
    #[serde(default)]
    extra_args: Vec<String>,
    #[serde(default)]
    env: HashMap<String, String>,
    #[serde(default)]
    permission_mode: Option<String>,
}

fn default_binary() -> String {
    "claude".into()
}

fn default_model() -> String {
    "claude-sonnet-4-6".into()
}

fn default_output_format() -> String {
    "json".into()
}

#[derive(Clone)]
pub struct AiCommandProvider;

impl Provider for AiCommandProvider {
    fn name(&self) -> String {
        "ai-command".into()
    }

    fn metadata(&self) -> ProviderMetadata {
        ProviderMetadata {
            name: self.name(),
            actions: vec![
                ActionMetadata::new("execute", "Run an AI command via shell")
                    .with_parameters(vec![
                        ParameterMetadata::required("command", ParameterValueType::String),
                        ParameterMetadata::optional("input", ParameterValueType::Json),
                    ])
                    .with_results(vec![ResultMetadata::new(
                        "response",
                        ParameterValueType::Json,
                    )]),
                ActionMetadata::new(
                    "claude_code",
                    "Invoke Claude Code non-interactively with a prompt and model",
                )
                .with_parameters(vec![
                    ParameterMetadata::required("prompt", ParameterValueType::String),
                    ParameterMetadata::optional("model", ParameterValueType::String)
                        .with_default(json!(default_model())),
                    ParameterMetadata::optional("binary", ParameterValueType::String)
                        .with_default(json!(default_binary())),
                    ParameterMetadata::optional("working_dir", ParameterValueType::String),
                    ParameterMetadata::optional("allowed_tools", ParameterValueType::String),
                    ParameterMetadata::optional("output_format", ParameterValueType::String)
                        .with_default(json!(default_output_format())),
                    ParameterMetadata::optional("permission_mode", ParameterValueType::String),
                    ParameterMetadata::optional("extra_args", ParameterValueType::Json),
                    ParameterMetadata::optional("env", ParameterValueType::Json),
                ])
                .with_results(vec![ResultMetadata::new(
                    "response",
                    ParameterValueType::Json,
                )]),
            ],
            metadata: ProviderRuntimeMetadata {
                credential_scopes: Vec::new(),
                contract: Some("stdin/stdout JSON".into()),
            },
        }
    }

    fn execute_service(
        &self,
        request: ProviderExecutionRequest,
        sink: Option<Arc<dyn ProviderEventSink>>,
    ) -> Result<TaskExecutionResult, SendableError> {
        match request.action_function.as_str() {
            "claude_code" => run_claude_code(&request, sink),
            // legacy default: shell-command execution.
            _ => run_shell_command(&request),
        }
    }
}

fn run_shell_command(
    request: &ProviderExecutionRequest,
) -> Result<TaskExecutionResult, SendableError> {
    let params: AiCommandParams = parse_params(request)?;
    let input = params.input.unwrap_or_else(|| json!({}));
    let mut child = Command::new("sh")
        .arg("-c")
        .arg(&params.command)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()?;
    if let Some(stdin) = child.stdin.as_mut() {
        stdin.write_all(serde_json::to_string(&input)?.as_bytes())?;
    }
    let output = child.wait_with_output()?;
    if !output.status.success() {
        return Err(Box::new(RuntimeError::new(
            "ai_command.nonzero_exit".into(),
            String::from_utf8_lossy(&output.stderr).to_string(),
        )));
    }
    let stdout = String::from_utf8(output.stdout)?;
    let parsed: Value = serde_json::from_str(&stdout).map_err(|err| {
        RuntimeError::new(
            "ai_command.invalid_json".into(),
            format!("AI command stdout must be JSON: {err}"),
        )
    })?;
    Ok(TaskExecutionResult {
        message: Some("AI command completed".into()),
        output_json: Some(parsed),
        chunks: Vec::new(),
        artifacts: Vec::new(),
    })
}

fn run_claude_code(
    request: &ProviderExecutionRequest,
    sink: Option<Arc<dyn ProviderEventSink>>,
) -> Result<TaskExecutionResult, SendableError> {
    let params: ClaudeCodeParams = parse_params(request)?;
    let argv = build_claude_argv(&params);

    let mut command = Command::new(&params.binary);
    command
        .args(&argv)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    if let Some(dir) = params.working_dir.as_deref() {
        command.current_dir(dir);
    }
    for (key, value) in &params.env {
        command.env(key, value);
    }

    let mut child = command.spawn().map_err(|err| {
        RuntimeError::new(
            "ai_command.claude_code.spawn".into(),
            format!("failed to spawn {}: {err}", params.binary),
        )
    })?;

    let stdout_handle = drain_stdout(&mut child, sink.as_ref());
    let output = child.wait_with_output().map_err(|err| {
        RuntimeError::new(
            "ai_command.claude_code.wait".into(),
            format!("failed to wait for claude: {err}"),
        )
    })?;
    let stdout = stdout_handle
        .map(|handle| handle.join().unwrap_or_default())
        .unwrap_or_else(|| String::from_utf8_lossy(&output.stdout).into_owned());

    if !output.status.success() {
        return Err(Box::new(RuntimeError::new(
            "ai_command.claude_code.exit_code".into(),
            format!(
                "claude exited with {}: {}",
                output.status,
                String::from_utf8_lossy(&output.stderr)
            ),
        )));
    }

    let parsed = parse_claude_output(&params.output_format, &stdout)?;
    Ok(TaskExecutionResult {
        message: Some("Claude Code completed".into()),
        output_json: Some(parsed),
        chunks: Vec::new(),
        artifacts: Vec::new(),
    })
}

fn build_claude_argv(params: &ClaudeCodeParams) -> Vec<String> {
    let mut argv = vec![
        "-p".into(),
        "--model".into(),
        params.model.clone(),
        "--output-format".into(),
        params.output_format.clone(),
    ];
    if let Some(tools) = params.allowed_tools.as_deref() {
        argv.push("--allowedTools".into());
        argv.push(tools.into());
    }
    if let Some(mode) = params.permission_mode.as_deref() {
        argv.push("--permission-mode".into());
        argv.push(mode.into());
    }
    for arg in &params.extra_args {
        argv.push(arg.clone());
    }
    // prompt is the trailing positional argument.
    argv.push(params.prompt.clone());
    argv
}

fn drain_stdout(
    child: &mut Child,
    sink: Option<&Arc<dyn ProviderEventSink>>,
) -> Option<JoinHandle<String>> {
    let stdout: ChildStdout = child.stdout.take()?;
    let sink = sink.cloned();
    let handle = thread::spawn(move || {
        let mut accumulator = String::new();
        let reader = BufReader::new(stdout);
        for line in reader.lines().map_while(Result::ok) {
            if let Some(sink) = sink.as_ref() {
                sink.emit(ProviderExecutionEvent::Chunk {
                    stream: "stdout".into(),
                    content: format!("{line}\n"),
                });
            }
            accumulator.push_str(&line);
            accumulator.push('\n');
        }
        accumulator
    });
    Some(handle)
}

fn parse_claude_output(format: &str, stdout: &str) -> Result<Value, SendableError> {
    match format {
        "json" | "stream-json" => serde_json::from_str::<Value>(stdout).map_err(|err| {
            Box::new(RuntimeError::new(
                "ai_command.claude_code.invalid_json".into(),
                format!("claude stdout was not valid JSON ({format}): {err}"),
            )) as SendableError
        }),
        _ => Ok(json!({ "text": stdout })),
    }
}

fn parse_params<T: DeserializeOwned>(
    request: &ProviderExecutionRequest,
) -> Result<T, SendableError> {
    serde_json::from_value(request.parameters.clone()).map_err(|e| {
        Box::new(RuntimeError::new(
            "ai_command.invalid_params".into(),
            e.to_string(),
        )) as SendableError
    })
}

#[cfg(test)]
mod tests;
