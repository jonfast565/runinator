use std::{process::Command, sync::Arc, time::Duration};

use runinator_models::errors::SendableError;
use runinator_plugin::cancel::CancellationToken;
use runinator_plugin::provider::ProviderEventSink;
use runinator_provider_support::process_runner::{ProcessFailure, ProcessRequest, ProcessRunner};

use crate::errors::{CANCELED, NONZERO_EXIT, TIMEOUT};

pub(crate) struct CommandOutput {
    pub success: bool,
    pub stdout: String,
    pub stderr: String,
}

pub(crate) fn run_command(
    runner: &dyn ProcessRunner,
    program: &str,
    args: &[&str],
    timeout_secs: i64,
    token: &CancellationToken,
    sink: Option<&Arc<dyn ProviderEventSink>>,
) -> Result<String, SendableError> {
    let output = run_command_output(runner, program, args, timeout_secs, token, sink)?;
    if !output.success {
        return Err(NONZERO_EXIT.error(output.stderr));
    }
    Ok(output.stdout)
}

pub(crate) fn run_command_output(
    runner: &dyn ProcessRunner,
    program: &str,
    args: &[&str],
    timeout_secs: i64,
    token: &CancellationToken,
    sink: Option<&Arc<dyn ProviderEventSink>>,
) -> Result<CommandOutput, SendableError> {
    let timeout = Duration::from_secs(timeout_secs.max(1) as u64);
    let mut command = Command::new(program);
    command.args(args);
    let result = runner
        .run(ProcessRequest {
            command: &mut command,
            input: None,
            timeout,
            cancellation: token,
            sink: sink.cloned(),
        })
        .map_err(|error| match error {
            ProcessFailure::Canceled => CANCELED.error(format!("{program} command canceled")),
            ProcessFailure::TimedOut => TIMEOUT.error(format!(
                "{program} command timed out after {} seconds",
                timeout.as_secs()
            )),
            ProcessFailure::Spawn(error) | ProcessFailure::Io(error) => {
                Box::new(error) as SendableError
            }
        })?;
    Ok(CommandOutput {
        success: result.status.success(),
        stdout: result.output.stdout,
        stderr: result.output.stderr,
    })
}

#[cfg(test)]
#[path = "command_tests.rs"]
mod tests;
