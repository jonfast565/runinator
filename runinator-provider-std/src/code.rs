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
const IMAGE_KEY: &str = "image";
const CONTEXT_KEY: &str = "context";

struct CodeRequest {
    language: String,
    source: String,
    image: Option<String>,
    context: Value,
}

struct LanguageSpec {
    image: &'static str,
    filename: &'static str,
    command: Vec<String>,
}

pub(crate) fn execute_code(
    request: &ProviderExecutionRequest,
    sink: Option<Arc<dyn ProviderEventSink>>,
    token: CancellationToken,
) -> Result<TaskExecutionResult, SendableError> {
    let code = parse_request(request)?;
    let spec = language_spec(&code.language);
    let image = code.image.as_deref().unwrap_or(spec.image);
    let work_dir = prepare_work_dir(request, spec.filename, &code.source)?;
    let output = run_docker(
        image,
        &spec.command,
        spec.filename,
        &work_dir,
        &code.context,
        request.timeout_secs,
        token,
    )?;

    emit_output(&sink, "stdout", &output.stdout);
    emit_output(&sink, "stderr", &output.stderr);

    if !output.status.success() {
        return Err(CODE_FAILED.error(format!(
            "docker exited with code {}: {}",
            output.status.code().unwrap_or(-1),
            String::from_utf8_lossy(&output.stderr)
        )));
    }

    let stdout = String::from_utf8(output.stdout)
        .map_err(|err| INVALID_CODE.error(format!("code stdout must be utf-8: {err}")))?;
    let output_json = if stdout.trim().is_empty() {
        Value::Null
    } else {
        serde_json::from_str::<serde_json::Value>(&stdout)
            .map(Value::from)
            .map_err(|err| INVALID_CODE.error(format!("code stdout must be JSON: {err}")))?
    };

    Ok(TaskExecutionResult {
        message: Some(format!(
            "{} code completed in docker image {image}",
            code.language
        )),
        output_json: Some(output_json),
        chunks: Vec::new(),
        artifacts: Vec::new(),
    })
}

fn parse_request(request: &ProviderExecutionRequest) -> Result<CodeRequest, SendableError> {
    let language = string_param(request, LANGUAGE_KEY)?;
    let source = string_param(request, SOURCE_KEY)?;
    let image = request
        .parameters
        .get(IMAGE_KEY)
        .and_then(Value::as_str)
        .map(str::to_string);
    let context = request
        .parameters
        .get(CONTEXT_KEY)
        .cloned()
        .unwrap_or(Value::Null);
    Ok(CodeRequest {
        language,
        source,
        image,
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

fn language_spec(language: &str) -> LanguageSpec {
    match language {
        "python" | "py" => LanguageSpec {
            image: "python:3.12-alpine",
            filename: "main.py",
            command: vec!["python".into()],
        },
        "javascript" | "js" | "node" => LanguageSpec {
            image: "node:22-alpine",
            filename: "main.js",
            command: vec!["node".into()],
        },
        "bash" | "sh" => LanguageSpec {
            image: "bash:5.2-alpine",
            filename: "main.sh",
            command: vec!["bash".into()],
        },
        "ruby" | "rb" => LanguageSpec {
            image: "ruby:3.3-alpine",
            filename: "main.rb",
            command: vec!["ruby".into()],
        },
        "perl" | "pl" => LanguageSpec {
            image: "perl:5.40",
            filename: "main.pl",
            command: vec!["perl".into()],
        },
        "php" => LanguageSpec {
            image: "php:8.3-cli-alpine",
            filename: "main.php",
            command: vec!["php".into()],
        },
        other => LanguageSpec {
            image: "debian:stable-slim",
            filename: "main.code",
            command: vec![other.into()],
        },
    }
}

fn prepare_work_dir(
    request: &ProviderExecutionRequest,
    filename: &str,
    source: &str,
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
    Ok(work_dir)
}

fn run_docker(
    image: &str,
    command: &[String],
    filename: &str,
    work_dir: &Path,
    context: &Value,
    timeout_secs: i64,
    token: CancellationToken,
) -> Result<std::process::Output, SendableError> {
    let mount = format!("{}:/work:ro", work_dir.display());
    let container_name = format!("runinator-code-{}", Uuid::new_v4());
    let mut child = Command::new("docker")
        .args([
            "run",
            "--rm",
            "-i",
            "--name",
            &container_name,
            "-v",
            &mount,
            "-w",
            "/work",
            image,
        ])
        .args(command)
        .arg(format!("/work/{filename}"))
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|err| CODE_FAILED.error(format!("failed to start docker: {err}")))?;

    if let Some(mut stdin) = child.stdin.take() {
        let input = serde_json::to_string(context)
            .map_err(|err| INVALID_CODE.error(format!("failed to encode code context: {err}")))?;
        stdin
            .write_all(input.as_bytes())
            .map_err(|err| CODE_FAILED.error(format!("failed to write code stdin: {err}")))?;
    }

    wait_with_timeout(child, timeout_secs, token, &container_name)
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
