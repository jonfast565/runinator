#[cfg(target_os = "windows")]
use std::process::Child;

#[cfg(target_os = "windows")]
pub fn kill_console_windows(child: &mut Child) {
    use winapi::um::consoleapi::SetConsoleCtrlHandler;
    use winapi::um::wincon::{CTRL_C_EVENT, GenerateConsoleCtrlEvent};

    unsafe {
        // disable ctrl+c handling for this process so we don't kill ourselves.
        SetConsoleCtrlHandler(None, 1);

        // send ctrl+c to the group identified by child.id(); pid equals gpid when child uses detached_process.
        GenerateConsoleCtrlEvent(CTRL_C_EVENT, child.id());
    }

    if let Err(e) = child.kill() {
        log::warn!("Failed to kill child process: {}", e);
    }
}
