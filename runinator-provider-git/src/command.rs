use std::{
    process::{Child, Command, ExitStatus, Stdio},
    sync::Arc,
    thread,
    time::{Duration, Instant},
};

use runinator_models::errors::SendableError;
use runinator_plugin::cancel::CancellationToken;
use runinator_plugin::provider::ProviderEventSink;
use runinator_provider_support::process::ProcessOutputPump;

use crate::errors::{CANCELED, NONZERO_EXIT, TIMEOUT};

pub(crate) struct CommandOutput {
    pub success: bool,
    pub stdout: String,
    pub stderr: String,
}

pub(crate) fn run_command(
    program: &str,
    args: &[&str],
    timeout_secs: i64,
    token: &CancellationToken,
    sink: Option<&Arc<dyn ProviderEventSink>>,
) -> Result<String, SendableError> {
    let output = run_command_output(program, args, timeout_secs, token, sink)?;
    if !output.success {
        return Err(NONZERO_EXIT.error(output.stderr));
    }
    Ok(output.stdout)
}

pub(crate) fn run_command_output(
    program: &str,
    args: &[&str],
    timeout_secs: i64,
    token: &CancellationToken,
    sink: Option<&Arc<dyn ProviderEventSink>>,
) -> Result<CommandOutput, SendableError> {
    let timeout = Duration::from_secs(timeout_secs.max(1) as u64);
    let started = Instant::now();
    let mut child = Command::new(program)
        .args(args)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()?;
    let output = ProcessOutputPump::start(&mut child, sink.cloned())?;
    let status = wait_for_child(&mut child, program, timeout, started, token);
    let output = output.finish();
    let status = status?;

    Ok(CommandOutput {
        success: status.success(),
        stdout: output.stdout,
        stderr: output.stderr,
    })
}

fn wait_for_child(
    child: &mut Child,
    program: &str,
    timeout: Duration,
    started: Instant,
    token: &CancellationToken,
) -> Result<ExitStatus, SendableError> {
    loop {
        if token.is_cancelled() {
            let _ = child.kill();
            let _ = child.wait();
            return Err(CANCELED.error(format!("{program} command canceled")));
        }
        if started.elapsed() >= timeout {
            let _ = child.kill();
            let _ = child.wait();
            return Err(TIMEOUT.error(format!(
                "{program} command timed out after {} seconds",
                timeout.as_secs()
            )));
        }
        match child.try_wait() {
            Ok(Some(status)) => return Ok(status),
            Ok(None) => {}
            Err(error) => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(Box::new(error));
            }
        }
        thread::sleep(Duration::from_millis(100));
    }
}
