use std::{
    fs,
    io::Write,
    path::{Path, PathBuf},
    process::{Child, Command, Stdio},
    sync::Arc,
    thread,
    time::{Duration, Instant},
};

use runinator_models::{
    errors::SendableError,
    runs::{ProviderExecutionEvent, ProviderExecutionRequest, TaskExecutionResult},
    value::Value,
};
use runinator_plugin::{cancel::CancellationToken, provider::ProviderEventSink};
use uuid::Uuid;

use crate::errors::{CODE_FAILED, INVALID_CODE};

const LANGUAGE_KEY: &str = "language";
const SOURCE_KEY: &str = "source";
const CONTEXT_KEY: &str = "context";
const RUNTIME_KEY: &str = "runtime";
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
    let language = language_spec(&code.language)?;
    let work_dir = prepare_work_dir(request, language.filename, &code.source, &code.runtime)?;
    let output = run_docker(
        &code.runtime.image,
        language.canonical,
        &language.command,
        &work_dir,
        &code.context,
        request.timeout_secs,
        token,
    )?;

    emit_output(&sink, "stdout", &output.process.stdout);
    emit_output(&sink, "stderr", &output.process.stderr);

    if !output.process.status.success() {
        return Err(CODE_FAILED.error(format!(
            "docker exited with code {}: {}",
            output.process.status.code().unwrap_or(-1),
            String::from_utf8_lossy(&output.process.stderr)
        )));
    }

    let output_json = parse_code_output(&output.output_path, output.process.stdout)?;

    Ok(TaskExecutionResult {
        message: Some(format!(
            "{} code completed in docker image {}",
            language.canonical, code.runtime.image
        )),
        output_json: Some(output_json),
        chunks: Vec::new(),
        artifacts: Vec::new(),
    })
}

pub(crate) fn parse_code_output(
    output_path: &Path,
    stdout: Vec<u8>,
) -> Result<Value, SendableError> {
    if output_path.exists() {
        let output = fs::read_to_string(output_path)
            .map_err(|err| INVALID_CODE.error(format!("failed to read code output: {err}")))?;
        if !output.trim().is_empty() {
            return serde_json::from_str::<serde_json::Value>(&output)
                .map(Value::from)
                .map_err(|err| {
                    INVALID_CODE.error(format!("code output file must contain JSON: {err}"))
                });
        }
    }

    let stdout = String::from_utf8(stdout)
        .map_err(|err| INVALID_CODE.error(format!("code stdout must be utf-8: {err}")))?;
    if stdout.trim().is_empty() {
        return Ok(Value::Null);
    }
    serde_json::from_str::<serde_json::Value>(&stdout)
        .map(Value::from)
        .map_err(|err| INVALID_CODE.error(format!("code stdout must be JSON: {err}")))
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
    Ok(CodeRequest {
        language,
        source,
        runtime,
        context,
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

pub(crate) struct LanguageSpec {
    pub(crate) canonical: &'static str,
    pub(crate) filename: &'static str,
    pub(crate) command: Vec<String>,
}

pub(crate) fn language_spec(language: &str) -> Result<LanguageSpec, SendableError> {
    let spec = match language {
        "python" | "py" => LanguageSpec {
            canonical: "python",
            filename: "main.py",
            command: run_command("python /work/main.py"),
        },
        "javascript" | "js" | "node" => LanguageSpec {
            canonical: "javascript",
            filename: "main.js",
            command: run_command("node /work/main.js"),
        },
        "bash" | "sh" => LanguageSpec {
            canonical: "bash",
            filename: "main.sh",
            command: run_command("bash /work/main.sh"),
        },
        "ruby" | "rb" => LanguageSpec {
            canonical: "ruby",
            filename: "main.rb",
            command: run_command("ruby /work/main.rb"),
        },
        "perl" | "pl" => LanguageSpec {
            canonical: "perl",
            filename: "main.pl",
            command: run_command("perl /work/main.pl"),
        },
        "php" => LanguageSpec {
            canonical: "php",
            filename: "main.php",
            command: run_command("php /work/main.php"),
        },
        other => {
            return Err(INVALID_CODE.error(format!(
                "unsupported foreign language '{other}'; supported languages: python, javascript, bash, ruby, perl, php"
            )));
        }
    };
    Ok(spec)
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
    filename: &str,
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
    fs::write(work_dir.join(filename), source)
        .map_err(|err| INVALID_CODE.error(format!("failed to write code source: {err}")))?;
    fs::write(work_dir.join(SETUP_FILE), &runtime.setup_script)
        .map_err(|err| INVALID_CODE.error(format!("failed to write code setup script: {err}")))?;
    Ok(work_dir)
}

struct DockerOutput {
    process: std::process::Output,
    output_path: PathBuf,
}

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

    let work_mount = format!("{}:{WORK_DIR}:ro", work_dir.display());
    let runtime_mount = format!("{}:{RUNTIME_DIR}", runtime_dir.display());
    let container_name = format!("runinator-code-{}", Uuid::new_v4());
    let mut child = Command::new("docker")
        .args([
            "run",
            "--rm",
            "-i",
            "--name",
            &container_name,
            "-v",
            &work_mount,
            "-v",
            &runtime_mount,
            "-w",
            WORK_DIR,
            "-e",
            &format!("RUNINATOR_CONTEXT={RUNTIME_DIR}/{CONTEXT_FILE}"),
            "-e",
            &format!("RUNINATOR_OUTPUT={RUNTIME_DIR}/{OUTPUT_FILE}"),
            "-e",
            &format!("RUNINATOR_LANGUAGE={language}"),
            image,
        ])
        .args(command)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|err| CODE_FAILED.error(format!("failed to start docker: {err}")))?;

    if let Some(mut stdin) = child.stdin.take() {
        stdin
            .write_all(input.as_bytes())
            .map_err(|err| CODE_FAILED.error(format!("failed to write code stdin: {err}")))?;
    }

    let process = wait_with_timeout(child, timeout_secs, token, &container_name)?;
    Ok(DockerOutput {
        process,
        output_path,
    })
}

fn wait_with_timeout(
    mut child: Child,
    timeout_secs: i64,
    token: CancellationToken,
    container_name: &str,
) -> Result<std::process::Output, SendableError> {
    let timeout = Duration::from_secs(timeout_secs.max(1) as u64);
    let started = Instant::now();
    loop {
        if token.is_cancelled() {
            let _ = child.kill();
            let _ = child.wait();
            force_remove_container(container_name);
            return Err(CODE_FAILED.error("code execution canceled"));
        }
        if started.elapsed() >= timeout {
            let _ = child.kill();
            let _ = child.wait();
            force_remove_container(container_name);
            return Err(CODE_FAILED.error(format!(
                "code execution timed out after {} seconds",
                timeout.as_secs()
            )));
        }
        if child
            .try_wait()
            .map_err(|err| CODE_FAILED.error(format!("failed to wait for docker: {err}")))?
            .is_some()
        {
            return child.wait_with_output().map_err(|err| {
                CODE_FAILED.error(format!("failed to collect docker output: {err}"))
            });
        }
        thread::sleep(Duration::from_millis(100));
    }
}

fn force_remove_container(name: &str) {
    let _ = Command::new("docker")
        .args(["rm", "-f", name])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status();
}

fn emit_output(sink: &Option<Arc<dyn ProviderEventSink>>, stream: &str, bytes: &[u8]) {
    let Some(sink) = sink else {
        return;
    };
    for line in String::from_utf8_lossy(bytes).lines() {
        sink.emit(ProviderExecutionEvent::Chunk {
            stream: stream.to_string(),
            content: line.to_string(),
        });
    }
}
