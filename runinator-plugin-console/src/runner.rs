use log::{error, info, warn};
use runinator_models::runs::{
    ProviderExecutionEvent, ProviderExecutionRequest, ProviderExecutionResponse,
};
use std::ffi::c_int;
use std::fs::OpenOptions;
use std::io::{BufRead, BufReader, Write};
#[cfg(target_os = "windows")]
use std::os::windows::process::CommandExt;
use std::process::{Child, Command, ExitStatus, Stdio};
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};
use std::thread;
use std::time::{Duration, Instant};

#[cfg(not(target_os = "windows"))]
use crate::linux;
#[cfg(target_os = "windows")]
use crate::windows;

pub(crate) fn execute_request(
    request_path: &str,
    response_path: &str,
) -> Result<c_int, Box<dyn std::error::Error>> {
    let request_file = std::fs::File::open(request_path)?;
    let request: ProviderExecutionRequest = serde_json::from_reader(request_file)?;
    let command = request
        .parameters
        .get("command")
        .and_then(|value| value.as_str())
        .or_else(|| {
            request
                .parameters
                .get("args")
                .and_then(|value| value.as_str())
        })
        .ok_or("Console plugin requires a command parameter")?
        .to_string();

    info!(
        "Running action '{}' w/ command `{}`",
        request.action_function, command
    );
    let exit_code = execute_command(&command, request.timeout_secs, &request.events_jsonl_path)?;
    let response = ProviderExecutionResponse {
        message: Some(format!("Console command exited with code {exit_code}")),
        output_json: Some(serde_json::json!({
            "success": exit_code == 0,
            "exit_code": exit_code,
            "command": command,
        })),
        chunks: Vec::new(),
        artifacts: Vec::new(),
    };
    std::fs::write(response_path, serde_json::to_vec_pretty(&response)?)?;
    Ok(if exit_code == 0 { 0 } else { exit_code })
}

fn execute_command(
    args_str: &str,
    timeout_secs: i64,
    events_jsonl_path: &str,
) -> Result<c_int, Box<dyn std::error::Error>> {
    #[cfg(target_os = "windows")]
    let mut command = {
        let mut cmd = Command::new("cmd");
        cmd.args(["/C", args_str]).creation_flags(0x00000008);
        cmd
    };

    #[cfg(not(target_os = "windows"))]
    let mut command = {
        let mut cmd = Command::new("sh");
        cmd.args(["-c", args_str]);
        cmd
    };

    command.stdout(Stdio::piped()).stderr(Stdio::piped());

    let mut child = command
        .spawn()
        .map_err(|e| format!("Failed to spawn command: {}", e))?;

    let stdout = child.stdout.take().ok_or("Failed to capture stdout")?;
    let stderr = child.stderr.take().ok_or("Failed to capture stderr")?;

    let stop_flag = Arc::new(AtomicBool::new(false));
    let stdout_thread = spawn_output_thread(
        stdout,
        Arc::clone(&stop_flag),
        false,
        events_jsonl_path.to_string(),
    );
    let stderr_thread = spawn_output_thread(
        stderr,
        Arc::clone(&stop_flag),
        true,
        events_jsonl_path.to_string(),
    );

    let start = Instant::now();
    let exit_status = wait_for_child(
        &mut child,
        Duration::from_secs(timeout_secs.max(0) as u64),
        start,
    )?;

    stop_flag.store(true, Ordering::Relaxed);
    let _ = stdout_thread.join();
    let _ = stderr_thread.join();

    let exit_code = exit_status.code().unwrap_or(0);
    info!("Exit code: {}", exit_code);
    Ok(exit_code)
}

fn spawn_output_thread<R: std::io::Read + Send + 'static>(
    reader: R,
    stop_flag: Arc<AtomicBool>,
    is_stderr: bool,
    events_jsonl_path: String,
) -> thread::JoinHandle<()> {
    thread::spawn(move || {
        let buf_reader = BufReader::new(reader);
        for line in buf_reader.lines() {
            if stop_flag.load(Ordering::Relaxed) {
                break;
            }
            match line {
                Ok(l) => {
                    if is_stderr {
                        error!("{}", l);
                    } else {
                        info!("{}", l);
                    }
                    if !events_jsonl_path.is_empty() {
                        let event = ProviderExecutionEvent::Chunk {
                            stream: if is_stderr { "stderr" } else { "stdout" }.into(),
                            content: l,
                        };
                        append_event(&events_jsonl_path, &event);
                    }
                }
                Err(e) => {
                    error!(
                        "Error reading {}: {}",
                        if is_stderr { "stderr" } else { "stdout" },
                        e
                    );
                    break;
                }
            }
        }
    })
}

fn append_event(path: &str, event: &ProviderExecutionEvent) {
    let Ok(serialized) = serde_json::to_string(event) else {
        return;
    };
    let mut file = match OpenOptions::new().create(true).append(true).open(path) {
        Ok(file) => file,
        Err(err) => {
            error!("Failed opening plugin event file {}: {}", path, err);
            return;
        }
    };
    if let Err(err) = writeln!(file, "{serialized}") {
        error!("Failed writing plugin event: {}", err);
    }
}

fn wait_for_child(
    child: &mut Child,
    timeout: Duration,
    start: Instant,
) -> Result<ExitStatus, Box<dyn std::error::Error>> {
    loop {
        match child.try_wait() {
            Ok(Some(status)) => return Ok(status),
            Ok(None) => {
                if start.elapsed() >= timeout {
                    warn!("Child exceeded timeout! Killing process.");
                    kill_child_process(child);
                    // wait for the process to exit after the kill attempt.
                    return child.wait().map_err(|e| e.into());
                }
            }
            Err(e) => return Err(e.into()),
        }
        thread::sleep(Duration::from_millis(100));
    }
}

fn kill_child_process(child: &mut Child) {
    #[cfg(target_os = "windows")]
    {
        windows::kill_console_windows(child);
    }
    #[cfg(not(target_os = "windows"))]
    {
        linux::kill_console_other(child);
    }
}
