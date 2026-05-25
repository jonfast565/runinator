use std::{
    process::{Command, Stdio},
    thread,
    time::{Duration, Instant},
};

use runinator_models::errors::{RuntimeError, SendableError};
use runinator_plugin::cancel::CancellationToken;

pub(crate) fn run_command(
    program: &str,
    args: &[&str],
    timeout_secs: i64,
    token: &CancellationToken,
) -> Result<String, SendableError> {
    let timeout = Duration::from_secs(timeout_secs.max(1) as u64);
    let started = Instant::now();
    let mut child = Command::new(program)
        .args(args)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()?;
    loop {
        if token.is_cancelled() {
            let _ = child.kill();
            let _ = child.wait();
            return Err(Box::new(RuntimeError::new(
                "command.canceled".into(),
                format!("{program} command canceled"),
            )));
        }
        if started.elapsed() >= timeout {
            let _ = child.kill();
            let _ = child.wait();
            return Err(Box::new(RuntimeError::new(
                "command.timeout".into(),
                format!(
                    "{program} command timed out after {} seconds",
                    timeout.as_secs()
                ),
            )));
        }
        if child.try_wait()?.is_some() {
            break;
        }
        thread::sleep(Duration::from_millis(100));
    }

    let output = child.wait_with_output()?;
    if !output.status.success() {
        return Err(Box::new(RuntimeError::new(
            "command.nonzero_exit".into(),
            String::from_utf8_lossy(&output.stderr).to_string(),
        )));
    }
    Ok(String::from_utf8_lossy(&output.stdout).to_string())
}
