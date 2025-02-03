mod model;
mod windows;
mod linux;

use ctor::ctor;
use log::{error, info, warn};
use runinator_utilities::{ffiutils, logger};
use std::ffi::{c_char, c_int};
use std::io::{BufRead, BufReader};
use std::os::windows::process::CommandExt;
use std::process::{Command, Stdio};
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc,
};
use std::{thread, time::Duration};

const NAME: &str = "Console\0";

#[ctor]
fn constructor() {
    logger::setup_logger().expect("logger not set up");
}

#[no_mangle]
extern "C" fn runinator_marker() -> c_int {
    1
}

#[no_mangle]
extern "C" fn name() -> *const c_char {
    ffiutils::str_to_c_string(NAME)
}

#[no_mangle]
extern "C" fn call_service(action_function: *const c_char, args: *const c_char) -> c_int {
    let call_str = ffiutils::cstr_to_rust_string(action_function);
    let args_str = ffiutils::cstr_to_rust_string(args);

    info!("Running action '{}' w/ args `{}`", call_str, args_str);

    const TIMEOUT_SECONDS: u64 = 30;

    let mut command = Command::new("cmd");
    command
        .args(["/C", &args_str])
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .creation_flags(0x00000008);

    let mut child = command.spawn().unwrap();
    let child_stdout = child.stdout.take().unwrap();
    let child_stderr = child.stderr.take().unwrap();

    let stop_reading = Arc::new(AtomicBool::new(false));
    let stop_reading_stdout = stop_reading.clone();
    let stop_reading_stderr = stop_reading.clone();

    let stdout_thread = thread::spawn(move || {
        let reader = BufReader::new(child_stdout);
        for line in reader.lines() {
            if stop_reading_stdout.load(Ordering::Relaxed) {
                break;
            }
            match line {
                Ok(l) => info!("{}", l),
                Err(e) => {
                    error!("Error reading child stdout: {}", e);
                    break;
                }
            }
        }
    });

    let stderr_thread = thread::spawn(move || {
        let reader = BufReader::new(child_stderr);
        for line in reader.lines() {
            if stop_reading_stderr.load(Ordering::Relaxed) {
                break;
            }
            match line {
                Ok(l) => error!("{}", l),
                Err(e) => {
                    error!("Error reading child stderr: {}", e);
                    break;
                }
            }
        }
    });

    let start = std::time::Instant::now();
    let mut child_exit_status = None;

    loop {
        match child.try_wait() {
            Ok(Some(status)) => {
                child_exit_status = Some(status);
                break;
            }
            Ok(None) => {
                if start.elapsed() >= Duration::from_secs(TIMEOUT_SECONDS) {
                    warn!("Child exceeded timeout! Killing.");
                    
                    #[cfg(target_os = "windows")]
                    windows::kill_console_windows(&mut child);
                
                    #[cfg(not(target_os = "windows"))]
                    linux::kill_console_other(&mut child);

                    // Wait to reap the actual exit code
                    // child_exit_status = child.wait().ok();
                    child_exit_status = None;
                    break;
                }
            }
            Err(e) => {
                error!("Error attempting to wait on child: {}", e);
                break;
            }
        }
        thread::sleep(Duration::from_millis(100));
    }

    // signal threads to stop and join them
    stop_reading.store(true, Ordering::Relaxed);
    let _ = stdout_thread.join();
    let _ = stderr_thread.join();

    let exit_code = match child_exit_status {
        Some(xit) => match xit.code() {
            Some(code) => code,
            None => 0
        }
        None => 0
    };

    info!("Exit code: {}", exit_code);
    exit_code
}
