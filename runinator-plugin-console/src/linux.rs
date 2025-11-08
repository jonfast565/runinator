#[cfg(not(target_os = "windows"))]
use std::process::Child;

#[cfg(not(target_os = "windows"))]
pub fn kill_console_other(child: &mut Child) {
    use libc::{SIGINT, kill};

    let pid = child.id() as i32;

    // Try to emulate the CTRL+C behavior on Linux by sending SIGINT first.
    unsafe {
        if kill(pid, SIGINT) != 0 {
            let err = std::io::Error::last_os_error();
            log::warn!("Failed to send SIGINT to child process {}: {}", pid, err);
        }
    }

    if let Err(e) = child.kill() {
        log::warn!("Failed to kill child process: {}", e);
    }
}
