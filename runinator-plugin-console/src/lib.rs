mod model;

use std::{ffi::{c_char, c_int}, io::{BufRead, BufReader, Stderr}, process::{Command, Stdio}, thread, time};
use log::{error, info};
use runinator_utilities::{ffiutils, logger};

const NAME: &str = "Console\0"; // Null-terminated string

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
    logger::setup_logger().unwrap();
    
    let call_str: String = ffiutils::cstr_to_rust_string(action_function);
    let args_str: String = ffiutils::cstr_to_rust_string(args);

    info!("Running action '{}' w/ args `{}`", call_str, args_str);

    const TIMEOUT_SECONDS : i32 = 5;
    
    let mut command = Command::new("cmd");
    command.args(["/C", &args_str]);
    command.stdout(Stdio::piped());
    command.stderr(Stdio::piped());

    let mut child = command.spawn().unwrap();
    let stdout = child.stdout.take().unwrap();
    let stderr = child.stderr.take().unwrap();

    let timeout_thread = thread::spawn(move || {
        for _ in 0..TIMEOUT_SECONDS {
            if let Ok(Some(_)) = child.try_wait() {
                return;
            }
            thread::sleep(time::Duration::from_secs(1));
        }
        child.kill().unwrap();
    });

    let stdout_reader = BufReader::new(stdout);
    for line in stdout_reader.lines() {
        info!("{}", line.unwrap());
    }

    let stderr_reader = BufReader::new(stderr);
    for line in stderr_reader.lines() {
        error!("{}", line.unwrap());
    }

    timeout_thread.join().unwrap();
    0
}