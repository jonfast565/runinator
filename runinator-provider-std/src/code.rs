use std::{
    fs,
    path::{Path, PathBuf},
    sync::Arc,
    time::Duration,
};

use runinator_models::{
    errors::SendableError,
    runs::{ProviderExecutionEvent, ProviderExecutionRequest, TaskExecutionResult},
    types::RuninatorType,
    value::Value,
};
use runinator_plugin::{cancel::CancellationToken, provider::ProviderEventSink};
use runinator_sandbox::{
    ContainerRunner, ContainerSpec, DockerRunner, Mount, SandboxError, SandboxLimits,
};
use uuid::Uuid;

use crate::errors::{CODE_FAILED, INVALID_CODE};
use crate::foreign_languages::{ForeignLanguageAdapter, adapter_for};

const LANGUAGE_KEY: &str = "language";
const SOURCE_KEY: &str = "source";
const CONTEXT_KEY: &str = "context";
const RUNTIME_KEY: &str = "runtime";
pub(crate) const EXPECTED_OUTPUT_TYPE_KEY: &str = "expected_output_type";
const SETUP_FILE: &str = "setup.sh";
const CONTEXT_FILE: &str = "context.json";
const OUTPUT_FILE: &str = "output.json";
const RUNTIME_DIR: &str = "/runinator";
const WORK_DIR: &str = "/work";

struct CodeRequest {
    language: String,
    source: String,
    runtime: CodeRuntime,
    context: Value,
    expected_output_type: Option<RuninatorType>,
}

struct CodeRuntime {
    image: String,
    setup_script: String,
}

pub(crate) fn execute_code(
    request: &ProviderExecutionRequest,
    sink: Option<Arc<dyn ProviderEventSink>>,
    token: CancellationToken,
) -> Result<TaskExecutionResult, SendableError> {
    let code = parse_request(request)?;
    let language = adapter_for(&code.language)?;
    let work_dir = prepare_work_dir(request, language, &code.source, &code.runtime)?;
    let output = run_docker(
        &code.runtime.image,
        language.canonical(),
        &run_command(language.execute()),
        &work_dir,
        &code.context,
        request.timeout_secs,
        token,
    )?;

    emit_output(&sink, "stdout", &output.result.stdout);
    emit_output(&sink, "stderr", &output.result.stderr);

    if !output.result.succeeded() {
        return Err(CODE_FAILED.error(format!(
            "docker exited with code {}: {}",
            output.result.exit_code, output.result.stderr
        )));
    }

    let output_json = parse_code_output(&output.output_path)?;
    validate_code_output(&output_json, code.expected_output_type.as_ref())?;

    Ok(TaskExecutionResult {
        message: Some(format!(
            "{} code completed in docker image {}",
            language.canonical(),
            code.runtime.image
        )),
        output_json: Some(output_json),
        chunks: Vec::new(),
        artifacts: Vec::new(),
    })
}

pub(crate) fn parse_code_output(output_path: &Path) -> Result<Value, SendableError> {
    let output = fs::read_to_string(output_path)
        .map_err(|err| INVALID_CODE.error(format!("foreign code did not return JSON: {err}")))?;
    serde_json::from_str::<serde_json::Value>(&output)
        .map(Value::from)
        .map_err(|err| INVALID_CODE.error(format!("foreign code returned invalid JSON: {err}")))
}

pub(crate) fn validate_code_output(
    output: &Value,
    expected: Option<&RuninatorType>,
) -> Result<(), SendableError> {
    let Some(expected) = expected else {
        return Ok(());
    };
    expected.validate_value(output).map_err(|violation| {
        INVALID_CODE.error(violation.message_with_label("foreign compute result"))
    })
}

fn parse_request(request: &ProviderExecutionRequest) -> Result<CodeRequest, SendableError> {
    let language = string_param(request, LANGUAGE_KEY)?;
    let source = string_param(request, SOURCE_KEY)?;
    let runtime = runtime_param(request)?;
    let context = request
        .parameters
        .get(CONTEXT_KEY)
        .cloned()
        .unwrap_or(Value::Null);
    let expected_output_type = request
        .parameters
        .get(EXPECTED_OUTPUT_TYPE_KEY)
        .map(|value| value.decode::<RuninatorType>())
        .transpose()
        .map_err(|err| INVALID_CODE.error(format!("invalid expected output type: {err}")))?;
    Ok(CodeRequest {
        language,
        source,
        runtime,
        context,
        expected_output_type,
    })
}

fn string_param(request: &ProviderExecutionRequest, name: &str) -> Result<String, SendableError> {
    request
        .parameters
        .get(name)
        .and_then(Value::as_str)
        .map(str::to_string)
        .ok_or_else(|| INVALID_CODE.error(format!("missing string parameter '{name}'")))
}

fn runtime_param(request: &ProviderExecutionRequest) -> Result<CodeRuntime, SendableError> {
    let runtime = request
        .parameters
        .get(RUNTIME_KEY)
        .and_then(Value::as_object)
        .ok_or_else(|| INVALID_CODE.error("missing runtime config"))?;
    let image = runtime
        .get("image")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|image| !image.is_empty())
        .map(str::to_string)
        .ok_or_else(|| INVALID_CODE.error("runtime.image must be a non-empty string"))?;
    let setup_script = runtime
        .get("setup_script")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_string();
    Ok(CodeRuntime {
        image,
        setup_script,
    })
}

fn run_command(execute: &str) -> Vec<String> {
    vec![
        "bash".into(),
        "-lc".into(),
        format!(
            "set -euo pipefail; if [ -s {WORK_DIR}/{SETUP_FILE} ]; then bash {WORK_DIR}/{SETUP_FILE}; fi; exec {execute}"
        ),
    ]
}

fn prepare_work_dir(
    request: &ProviderExecutionRequest,
    language: &dyn ForeignLanguageAdapter,
    source: &str,
    runtime: &CodeRuntime,
) -> Result<PathBuf, SendableError> {
    let base = if request.artifact_dir.is_empty() {
        std::env::temp_dir().join("runinator-std-code")
    } else {
        PathBuf::from(&request.artifact_dir)
    };
    let work_dir = base.join("code").join(Uuid::new_v4().to_string());
    fs::create_dir_all(&work_dir)
        .map_err(|err| INVALID_CODE.error(format!("failed to create code work dir: {err}")))?;
    fs::write(work_dir.join(language.source_filename()), source)
        .map_err(|err| INVALID_CODE.error(format!("failed to write code source: {err}")))?;
    fs::write(
        work_dir.join(language.runner_filename()),
        language.runner_source(),
    )
    .map_err(|err| INVALID_CODE.error(format!("failed to write code runner: {err}")))?;
    for (filename, contents) in language.additional_files() {
        fs::write(work_dir.join(filename), contents).map_err(|err| {
            INVALID_CODE.error(format!(
                "failed to write code support file '{filename}': {err}"
            ))
        })?;
    }
    fs::write(work_dir.join(SETUP_FILE), &runtime.setup_script)
        .map_err(|err| INVALID_CODE.error(format!("failed to write code setup script: {err}")))?;
    Ok(work_dir)
}

struct DockerOutput {
    result: runinator_sandbox::ContainerOutput,
    output_path: PathBuf,
}

// container execution itself lives in `runinator-sandbox`, shared with packaged functions. what
// stays here is the `std.code` contract: the context/output file pair and the mount layout the
// language runners expect.
//
// the limits are deliberately `compatible` rather than the hardened default. an author's
// `setup_script` exists to install dependencies, so it needs both the network and a writable root;
// taking those away as a side effect of sharing a runner would break working snippets. what the
// port does change is that output is now bounded and drained concurrently — previously a snippet
// writing more than a pipe buffer deadlocked and died on its timeout.
fn run_docker(
    image: &str,
    language: &str,
    command: &[String],
    work_dir: &Path,
    context: &Value,
    timeout_secs: i64,
    token: CancellationToken,
) -> Result<DockerOutput, SendableError> {
    let runtime_dir = work_dir.join("runtime");
    fs::create_dir_all(&runtime_dir)
        .map_err(|err| INVALID_CODE.error(format!("failed to create code runtime dir: {err}")))?;
    let context_path = runtime_dir.join(CONTEXT_FILE);
    let output_path = runtime_dir.join(OUTPUT_FILE);
    let input = serde_json::to_string(context)
        .map_err(|err| INVALID_CODE.error(format!("failed to encode code context: {err}")))?;
    fs::write(&context_path, &input)
        .map_err(|err| INVALID_CODE.error(format!("failed to write code context: {err}")))?;

    let spec = ContainerSpec::new(image, "runinator-code")
        .with_command(command.to_vec())
        .with_working_dir(WORK_DIR)
        .with_mount(Mount::read_only(work_dir, WORK_DIR))
        .with_mount(Mount::writable(&runtime_dir, RUNTIME_DIR))
        .with_env("RUNINATOR_CONTEXT", format!("{RUNTIME_DIR}/{CONTEXT_FILE}"))
        .with_env("RUNINATOR_OUTPUT", format!("{RUNTIME_DIR}/{OUTPUT_FILE}"))
        .with_env("RUNINATOR_LANGUAGE", language)
        .with_stdin(input.into_bytes())
        .with_limits(SandboxLimits::compatible(Duration::from_secs(
            timeout_secs.max(1) as u64,
        )));

    let cancel = move || token.is_cancelled();
    let result = DockerRunner::new()
        .run(&spec, None, &cancel)
        .map_err(|err| match err {
            SandboxError::Cancelled => CODE_FAILED.error("code execution canceled"),
            SandboxError::TimedOut(seconds) => {
                CODE_FAILED.error(format!("code execution timed out after {seconds} seconds"))
            }
            other => CODE_FAILED.error(other.to_string()),
        })?;

    Ok(DockerOutput {
        result,
        output_path,
    })
}

fn emit_output(sink: &Option<Arc<dyn ProviderEventSink>>, stream: &str, text: &str) {
    let Some(sink) = sink else {
        return;
    };
    for line in text.lines() {
        sink.emit(ProviderExecutionEvent::Chunk {
            stream: stream.to_string(),
            content: line.to_string(),
        });
    }
}
