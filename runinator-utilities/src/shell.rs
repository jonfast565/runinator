use std::process::Command;

/// builds a shell command for `command_text`, using `cmd /c` on windows and `sh -c` elsewhere.
pub fn shell_command(command_text: &str) -> Command {
    #[cfg(target_os = "windows")]
    {
        let mut cmd = Command::new("cmd");
        cmd.args(["/C", command_text]);
        cmd
    }
    #[cfg(not(target_os = "windows"))]
    {
        let mut cmd = Command::new("sh");
        cmd.args(["-c", command_text]);
        cmd
    }
}
