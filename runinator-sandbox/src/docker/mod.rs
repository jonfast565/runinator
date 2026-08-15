//! the docker backend.

pub mod args;
pub mod pump;

use std::io::Write;
use std::process::{Child, Command, Stdio};
use std::sync::Arc;
use std::thread;
use std::time::{Duration, Instant};

use uuid::Uuid;

use crate::Result;
use crate::errors::SandboxError;
use crate::runner::{CancelSignal, ContainerRunner, LineSink, Stream};
use crate::spec::{ContainerOutput, ContainerSpec};

/// how often the run loop checks the deadline and the cancel signal.
const POLL_INTERVAL: Duration = Duration::from_millis(100);

/// runs containers through the local `docker` cli.
///
/// the cli rather than the daemon api on purpose: it is what is already present wherever runinator
/// runs a container today, it needs no socket permissions beyond what the operator already granted,
/// and the flags it takes are the documented surface rather than a library's rendering of them.
pub struct DockerRunner {
    binary: String,
}

impl Default for DockerRunner {
    fn default() -> Self {
        Self::new()
    }
}

impl DockerRunner {
    pub fn new() -> Self {
        Self {
            // podman is argv-compatible for everything used here, so an operator can point at it.
            binary: std::env::var("RUNINATOR_CONTAINER_BINARY").unwrap_or_else(|_| "docker".into()),
        }
    }

    pub fn with_binary(binary: impl Into<String>) -> Self {
        Self {
            binary: binary.into(),
        }
    }

    /// true when the container runtime answers at all. callers use this to fail a dispatch with a
    /// clear reason rather than a mangled spawn error.
    pub fn available(&self) -> bool {
        Command::new(&self.binary)
            .arg("version")
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .map(|status| status.success())
            .unwrap_or(false)
    }

    fn force_remove(&self, container_name: &str) {
        let _ = Command::new(&self.binary)
            .args(args::remove_args(container_name))
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status();
    }
}

impl ContainerRunner for DockerRunner {
    fn backend(&self) -> &'static str {
        "docker"
    }

    fn run(
        &self,
        spec: &ContainerSpec,
        logs: Option<Arc<dyn LineSink>>,
        cancel: &dyn CancelSignal,
    ) -> Result<ContainerOutput> {
        if spec.image.trim().is_empty() {
            return Err(SandboxError::InvalidSpec("image must not be empty".into()));
        }
        let container_name = format!("{}-{}", spec.name_prefix, Uuid::new_v4());
        let started = Instant::now();

        let mut child = Command::new(&self.binary)
            .args(args::run_args(spec, &container_name))
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|err| {
                SandboxError::RuntimeUnavailable(format!("failed to start {}: {err}", self.binary))
            })?;

        // the pumps start before stdin is written and before the wait loop: both streams have to be
        // draining for the whole life of the container, not just after it is expected to be done.
        let stdout = child.stdout.take().map(|stdout| {
            pump::spawn(
                stdout,
                Stream::Stdout,
                spec.limits.max_output_bytes,
                logs.clone(),
            )
        });
        let stderr = child.stderr.take().map(|stderr| {
            pump::spawn(
                stderr,
                Stream::Stderr,
                spec.limits.max_output_bytes,
                logs.clone(),
            )
        });

        if let Some(input) = &spec.stdin
            && let Some(mut handle) = child.stdin.take()
        {
            // a closed stdin is not an error: a payload is free to ignore what it is sent, and
            // failing the run for that would be reporting the container's choice as our fault.
            let _ = handle.write_all(input);
        }
        // dropping our end signals eof, without which a payload reading to end-of-input hangs.
        drop(child.stdin.take());

        let exit = self.wait(&mut child, spec, cancel, &container_name);

        let stdout = stdout
            .and_then(|handle| handle.join().ok())
            .unwrap_or_default();
        let stderr = stderr
            .and_then(|handle| handle.join().ok())
            .unwrap_or_default();
        let exit_code = exit?;

        Ok(ContainerOutput {
            exit_code,
            stdout: stdout.text,
            stderr: stderr.text,
            stdout_truncated: stdout.truncated,
            stderr_truncated: stderr.truncated,
            duration: started.elapsed(),
        })
    }
}

impl DockerRunner {
    // poll for exit, the deadline, and cancellation. the deadline is enforced here rather than
    // handed to the container, because a payload that ignores its own timeout is exactly the case
    // this has to survive.
    fn wait(
        &self,
        child: &mut Child,
        spec: &ContainerSpec,
        cancel: &dyn CancelSignal,
        container_name: &str,
    ) -> Result<i32> {
        let timeout = spec.limits.timeout.max(Duration::from_secs(1));
        let started = Instant::now();
        loop {
            if cancel.is_cancelled() {
                self.abort(child, container_name);
                return Err(SandboxError::Cancelled);
            }
            if started.elapsed() >= timeout {
                self.abort(child, container_name);
                return Err(SandboxError::TimedOut(timeout.as_secs()));
            }
            match child.try_wait() {
                Ok(Some(status)) => return Ok(status.code().unwrap_or(-1)),
                Ok(None) => thread::sleep(POLL_INTERVAL),
                Err(err) => {
                    self.abort(child, container_name);
                    return Err(SandboxError::Io(format!(
                        "failed to wait for docker: {err}"
                    )));
                }
            }
        }
    }

    // killing the cli client does not stop the container it asked for, so the removal is what
    // actually ends the run; without it an aborted execution keeps consuming the limits it was
    // given until it finishes on its own.
    fn abort(&self, child: &mut Child, container_name: &str) {
        self.force_remove(container_name);
        let _ = child.kill();
        let _ = child.wait();
    }
}

#[cfg(test)]
#[path = "mod_tests.rs"]
mod tests;
