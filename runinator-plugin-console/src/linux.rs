use std::process::Child;

#[cfg(not(target_os = "windows"))]
pub fn kill_console_other(child: &mut Child) {
    use std::process::Child;
    let _ = child.kill().expect("command couldn't be killed");
}