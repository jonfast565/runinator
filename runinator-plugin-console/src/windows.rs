use std::process::Child;

#[cfg(target_os = "windows")]
pub fn kill_console_windows(child: &mut Child) {
    use winapi::um::consoleapi::SetConsoleCtrlHandler;
    use winapi::um::wincon::GenerateConsoleCtrlEvent;

    unsafe {
        SetConsoleCtrlHandler(None, 1);
        GenerateConsoleCtrlEvent(winapi::um::wincon::CTRL_C_EVENT, child.id());
    }

    let _ = child.kill().expect("command couldn't be killed");
}
