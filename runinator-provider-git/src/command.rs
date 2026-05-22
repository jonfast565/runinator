use std::process::Command;

use runinator_models::errors::{RuntimeError, SendableError};

pub(crate) fn run_command(program: &str, args: &[&str]) -> Result<String, SendableError> {
    let output = Command::new(program).args(args).output()?;
    if !output.status.success() {
        return Err(Box::new(RuntimeError::new(
            "command.nonzero_exit".into(),
            String::from_utf8_lossy(&output.stderr).to_string(),
        )));
    }
    Ok(String::from_utf8_lossy(&output.stdout).to_string())
}
