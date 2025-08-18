#[cfg(not(target_os = "windows"))]
use std::process::Child;

#[cfg(not(target_os = "windows"))]
pub fn kill_console_other(child: &mut Child) {
    if let Err(e) = child.kill() {
        log::warn!("Failed to kill child process: {}", e);
    }
}
