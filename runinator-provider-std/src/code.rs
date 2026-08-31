use std::{
    collections::BTreeMap,
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
    ContainerRunner, ContainerSpec, DockerRunner, LineSink, Mount, SandboxError, SandboxLimits,
    Stream,
};
use uuid::Uuid;

use crate::errors::{CODE_FAILED, INVALID_CODE};
use crate::foreign_languages::{ForeignLanguageAdapter, ToolchainConfig, adapter_for};

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
    environment: BTreeMap<String, String>,
    executable: Option<String>,
    build_args: Vec<String>,
    run_args: Vec<String>,
    limits: RuntimeLimits,
}

#[derive(Debug, Clone)]
struct RuntimeLimits {
    memory_mb: i64,
    cpu_millis: i64,
    pids: i64,
    tmpfs_mb: i64,
    max_output_bytes: usize,
}

struct DockerRun<'a> {
    image: &'a str,
    language: &'a str,
    command: &'a [String],
    work_dir: &'a Path,
    context: &'a Value,
    timeout_secs: i64,
    environment: &'a BTreeMap<String, String>,
    limits: &'a RuntimeLimits,
}

pub(crate) fn execute_code(
    request: &ProviderExecutionRequest,
    sink: Option<Arc<dyn ProviderEventSink>>,
    token: CancellationToken,
) -> Result<TaskExecutionResult, SendableError> {
    let code = parse_request(request)?;
    let language = adapter_for(&code.language)?;
    let toolchain = ToolchainConfig {
        executable: code
            .runtime
            .executable
            .clone()
            .unwrap_or_else(|| language.default_executable().to_string()),
        build_args: code.runtime.build_args.clone(),
        run_args: code.runtime.run_args.clone(),
    };
    let work_dir = prepare_work_dir(request, language, &code.source, &code.runtime, &toolchain)?;
    let command = run_command(&language.rendered_execute(&toolchain));
    let output = run_docker(
        DockerRun {
            image: &code.runtime.image,
            language: language.canonical(),
            command: &command,
            work_dir: &work_dir,
            context: &code.context,
            timeout_secs: request.timeout_secs,
            environment: &code.runtime.environment,
            limits: &code.runtime.limits,
        },
        sink,
        token,
    )?;

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
    let environment = parse_environment(runtime.get("environment"))?;
    let toolchain = optional_object(runtime.get("toolchain"), "runtime.toolchain")?;
    let executable = toolchain
        .and_then(|value| value.get("executable"))
        .map(|value| {
            value
                .as_str()
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .map(str::to_string)
                .ok_or_else(|| {
                    INVALID_CODE.error("runtime.toolchain.executable must be a non-empty string")
                })
        })
        .transpose()?;
    let build_args = parse_string_array(
        toolchain.and_then(|value| value.get("build_args")),
        "runtime.toolchain.build_args",
    )?;
    let run_args = parse_string_array(
        toolchain.and_then(|value| value.get("run_args")),
        "runtime.toolchain.run_args",
    )?;
    let limits_object = optional_object(runtime.get("limits"), "runtime.limits")?;
    let limits = RuntimeLimits {
        memory_mb: positive_i64(limits_object, "memory_mb", 2048)?,
        cpu_millis: positive_i64(limits_object, "cpu_millis", 2000)?,
        pids: positive_i64(limits_object, "pids", 256)?,
        tmpfs_mb: positive_i64(limits_object, "tmpfs_mb", 512)?,
        max_output_bytes: positive_i64(limits_object, "max_output_bytes", 1024 * 1024)?
            .try_into()
            .map_err(|_| INVALID_CODE.error("runtime.limits.max_output_bytes is too large"))?,
    };
    Ok(CodeRuntime {
        image,
        setup_script,
        environment,
        executable,
        build_args,
        run_args,
        limits,
    })
}

fn optional_object<'a>(
    value: Option<&'a Value>,
    field: &str,
) -> Result<Option<&'a runinator_models::value::Map>, SendableError> {
    value
        .map(|value| {
            value
                .as_object()
                .ok_or_else(|| INVALID_CODE.error(format!("{field} must be an object")))
        })
        .transpose()
}

fn parse_environment(value: Option<&Value>) -> Result<BTreeMap<String, String>, SendableError> {
    let Some(environment) = optional_object(value, "runtime.environment")? else {
        return Ok(BTreeMap::new());
    };
    let mut parsed = BTreeMap::new();
    for (name, value) in environment {
        if !valid_environment_name(name) {
            return Err(INVALID_CODE.error(format!(
                "runtime.environment contains invalid variable name '{name}'"
            )));
        }
        if matches!(
            name.as_str(),
            "RUNINATOR_CONTEXT" | "RUNINATOR_OUTPUT" | "RUNINATOR_LANGUAGE"
        ) {
            return Err(INVALID_CODE.error(format!(
                "runtime.environment cannot override reserved variable '{name}'"
            )));
        }
        let value = value.as_str().ok_or_else(|| {
            INVALID_CODE.error(format!("runtime.environment.{name} must be a string"))
        })?;
        parsed.insert(name.clone(), value.to_string());
    }
    Ok(parsed)
}

fn valid_environment_name(name: &str) -> bool {
    let mut chars = name.chars();
    matches!(chars.next(), Some('_' | 'A'..='Z' | 'a'..='z'))
        && chars.all(|character| matches!(character, '_' | 'A'..='Z' | 'a'..='z' | '0'..='9'))
}

fn parse_string_array(value: Option<&Value>, field: &str) -> Result<Vec<String>, SendableError> {
    let Some(value) = value else {
        return Ok(Vec::new());
    };
    let values = value
        .as_array()
        .ok_or_else(|| INVALID_CODE.error(format!("{field} must be an array of strings")))?;
    values
        .iter()
        .map(|value| {
            value
                .as_str()
                .map(str::to_string)
                .ok_or_else(|| INVALID_CODE.error(format!("{field} must contain only strings")))
        })
        .collect()
}

fn positive_i64(
    object: Option<&runinator_models::value::Map>,
    name: &str,
    default: i64,
) -> Result<i64, SendableError> {
    let Some(value) = object.and_then(|object| object.get(name)) else {
        return Ok(default);
    };
    value.as_i64().filter(|value| *value > 0).ok_or_else(|| {
        INVALID_CODE.error(format!("runtime.limits.{name} must be a positive integer"))
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
    toolchain: &ToolchainConfig,
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
        language.rendered_runner_source(toolchain),
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
    run: DockerRun<'_>,
    sink: Option<Arc<dyn ProviderEventSink>>,
    token: CancellationToken,
) -> Result<DockerOutput, SendableError> {
    let runtime_dir = run.work_dir.join("runtime");
    fs::create_dir_all(&runtime_dir)
        .map_err(|err| INVALID_CODE.error(format!("failed to create code runtime dir: {err}")))?;
    let context_path = runtime_dir.join(CONTEXT_FILE);
    let output_path = runtime_dir.join(OUTPUT_FILE);
    let input = serde_json::to_string(run.context)
        .map_err(|err| INVALID_CODE.error(format!("failed to encode code context: {err}")))?;
    fs::write(&context_path, &input)
        .map_err(|err| INVALID_CODE.error(format!("failed to write code context: {err}")))?;

    let limits = SandboxLimits {
        memory_mb: Some(run.limits.memory_mb),
        cpu_millis: Some(run.limits.cpu_millis),
        pids: Some(run.limits.pids),
        tmpfs_mb: Some(run.limits.tmpfs_mb),
        max_output_bytes: run.limits.max_output_bytes,
        ..SandboxLimits::compatible(Duration::from_secs(run.timeout_secs.max(1) as u64))
    };
    let mut spec = ContainerSpec::new(run.image, "runinator-code")
        .with_command(run.command.to_vec())
        .with_working_dir(WORK_DIR)
        .with_mount(Mount::read_only(run.work_dir, WORK_DIR))
        .with_mount(Mount::writable(&runtime_dir, RUNTIME_DIR))
        .with_stdin(input.into_bytes())
        .with_limits(limits);
    for (key, value) in run.environment {
        spec = spec.with_env(key, value);
    }
    spec = spec
        .with_env("RUNINATOR_CONTEXT", format!("{RUNTIME_DIR}/{CONTEXT_FILE}"))
        .with_env("RUNINATOR_OUTPUT", format!("{RUNTIME_DIR}/{OUTPUT_FILE}"))
        .with_env("RUNINATOR_LANGUAGE", run.language);

    let cancel = move || token.is_cancelled();
    let logs = sink.map(|sink| Arc::new(EventLineSink(sink)) as Arc<dyn LineSink>);
    let result = DockerRunner::new()
        .run(&spec, logs, &cancel)
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

struct EventLineSink(Arc<dyn ProviderEventSink>);

impl LineSink for EventLineSink {
    fn line(&self, stream: Stream, text: &str) {
        self.0.emit(ProviderExecutionEvent::Chunk {
            stream: stream.as_str().to_string(),
            content: text.to_string(),
        });
    }
}
