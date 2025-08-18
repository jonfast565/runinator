use std::process::Child;

#[cfg(target_os = "windows")]
pub fn kill_console_windows(child: &mut Child) {
    use winapi::um::consoleapi::SetConsoleCtrlHandler;
    use winapi::um::wincon::{CTRL_C_EVENT, GenerateConsoleCtrlEvent};

    unsafe {
        // Disable Ctrl+C handling for this process so we don’t kill ourselves.
        SetConsoleCtrlHandler(None, 1);

        // Send CTRL+C event to the process group identified by child.id().
        // (Note: child.id() returns the process ID, which must be the same as
        // the process group ID if the child was created with DETACHED_PROCESS.)
        GenerateConsoleCtrlEvent(CTRL_C_EVENT, child.id());
    }

    if let Err(e) = child.kill() {
        log::warn!("Failed to kill child process: {}", e);
    }
}
