mod linux;
mod model;
mod windows;

use ctor::ctor;
use log::{error, info, warn};
use runinator_utilities::{ffiutils, logger};
use std::ffi::{c_char, c_int};
use std::io::{BufRead, BufReader};
use std::os::windows::process::CommandExt;
use std::process::{Child, Command, ExitStatus, Stdio};
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};
use std::thread;
use std::time::{Duration, Instant};

const NAME: &str = "Console\0";

#[ctor]
fn constructor() {
    logger::setup_logger().expect("logger not set up");
}

#[unsafe(no_mangle)]
pub extern "C" fn runinator_marker() -> c_int {
    1
}

#[unsafe(no_mangle)]
pub extern "C" fn name() -> *const c_char {
    ffiutils::str_to_c_string(NAME)
}

#[unsafe(no_mangle)]
pub extern "C" fn call_service(action_function: *const c_char, args: *const c_char, timeout_secs: i64) -> c_int {
    let action = ffiutils::cstr_to_rust_string(action_function);
    let args_str = ffiutils::cstr_to_rust_string(args);

    info!("Running action '{}' w/ args `{}`", action, args_str);

    execute_command(&args_str, timeout_secs).unwrap_or_else(|e| {
        error!("Error executing command: {}", e);
        -1
    })
}

fn execute_command(args_str: &str, timeout_secs: i64) -> Result<c_int, Box<dyn std::error::Error>> {
    let mut command = Command::new("cmd");
    command
        .args(["/C", args_str])
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .creation_flags(0x00000008);

    let mut child = command
        .spawn()
        .map_err(|e| format!("Failed to spawn command: {}", e))?;

    let stdout = child.stdout.take().ok_or("Failed to capture stdout")?;
    let stderr = child.stderr.take().ok_or("Failed to capture stderr")?;

    let stop_flag = Arc::new(AtomicBool::new(false));
    let stdout_thread = spawn_output_thread(stdout, Arc::clone(&stop_flag), false);
    let stderr_thread = spawn_output_thread(stderr, Arc::clone(&stop_flag), true);

    let start = Instant::now();
    let exit_status = wait_for_child(&mut child, Duration::from_secs(timeout_secs.max(0) as u64), start)?;

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
                    // Wait for the process to exit after the kill attempt.
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
