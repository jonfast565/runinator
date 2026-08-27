//! where an invocation actually executes.
//!
//! the seam is here rather than in `runinator-sandbox` because *this* is the decision that varies:
//! a host worker runs a container locally, and a kubernetes worker will one day submit a Job. both
//! are "run this packaged function"; only one of them is `docker run`.

use std::fs;
use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

#[cfg(unix)]
use std::os::unix::fs::PermissionsExt;

use runinator_models::errors::SendableError;
use runinator_models::value::Value;
use runinator_plugin::cancel::CancellationToken;
use runinator_sandbox::{
    ContainerRunner, ContainerSpec, DockerRunner, LineSink, Mount, SandboxError, SandboxLimits,
};
use uuid::Uuid;

use crate::errors::{
    INVALID_OUTPUT, INVOCATION_CANCELED, INVOCATION_FAILED, INVOCATION_TIMEOUT, PACKAGE_UNREADABLE,
    RUNTIME_UNAVAILABLE,
};
use crate::languages::{adapter_for, default_image};
use crate::request::InvocationRequest;

/// where the package is mounted inside the container.
const PACKAGE_DIR: &str = "/package";
/// where the shim, the input, and the output live. writable; the package mount is not.
const RUNTIME_DIR: &str = "/runinator";
const INPUT_FILE: &str = "input.json";
const OUTPUT_FILE: &str = "output.json";
const SETUP_FILE: &str = "setup.sh";

/// what one invocation produced.
#[derive(Debug, Clone)]
pub struct InvocationOutcome {
    pub output: Value,
    pub stdout: String,
    pub stderr: String,
    pub truncated: bool,
    pub duration: Duration,
}

/// executes one packaged-function invocation.
pub trait InvocationRuntime: Send + Sync {
    fn name(&self) -> &'static str;

    fn invoke(
        &self,
        request: &InvocationRequest,
        logs: Option<Arc<dyn LineSink>>,
        token: CancellationToken,
    ) -> Result<InvocationOutcome, SendableError>;
}

/// runs an invocation in a local container.
pub struct DockerInvocationRuntime {
    runner: DockerRunner,
}

impl Default for DockerInvocationRuntime {
    fn default() -> Self {
        Self::new()
    }
}

impl DockerInvocationRuntime {
    pub fn new() -> Self {
        Self {
            runner: DockerRunner::new(),
        }
    }

    pub fn available(&self) -> bool {
        self.runner.available()
    }
}

impl InvocationRuntime for DockerInvocationRuntime {
    fn name(&self) -> &'static str {
        "docker"
    }

    fn invoke(
        &self,
        request: &InvocationRequest,
        logs: Option<Arc<dyn LineSink>>,
        token: CancellationToken,
    ) -> Result<InvocationOutcome, SendableError> {
        let (adapter, _) = adapter_for(&request.runtime.runtime)?;
        // an explicit image overrides the runtime's default, which is how a package pins a base it
        // has already tested against.
        let image = match &request.runtime.image {
            Some(image) if !image.trim().is_empty() => image.clone(),
            _ => default_image(&request.runtime.runtime)?,
        };

        // the runtime directory is per-invocation and sits beside the staged package rather than
        // inside it: the package mount is read-only and shared by every concurrent invocation of
        // the same digest, so nothing may write into it.
        let runtime_dir = staging_dir(&request.package_path)?;
        write_file(
            &runtime_dir.join(adapter.shim_filename()),
            adapter.shim_source().as_bytes(),
        )?;
        let payload = runinator_models::json!({
            "input": request.input.clone(),
            "context": request.context.clone(),
        });
        let encoded = serde_json::to_vec(&payload)
            .map_err(|err| INVOCATION_FAILED.error(format!("failed to encode input: {err}")))?;
        write_file(&runtime_dir.join(INPUT_FILE), &encoded)?;
        // pre-created so a handler returning nothing still leaves a readable (empty) file rather
        // than a missing-file error that reads like the container never ran.
        write_output_file(&runtime_dir.join(OUTPUT_FILE))?;

        let command = match &request.runtime.setup_script {
            Some(script) if !script.trim().is_empty() => {
                write_file(&runtime_dir.join(SETUP_FILE), script.as_bytes())?;
                setup_wrapped_command(adapter.command(RUNTIME_DIR))
            }
            _ => adapter.command(RUNTIME_DIR),
        };

        let spec = ContainerSpec::new(image, "runinator-function")
            .with_command(command)
            .with_working_dir(PACKAGE_DIR)
            .with_mount(Mount::read_only(&request.package_path, PACKAGE_DIR))
            .with_mount(Mount::writable(&runtime_dir, RUNTIME_DIR))
            .with_env("RUNINATOR_PACKAGE", PACKAGE_DIR)
            .with_env("RUNINATOR_HANDLER", &request.handler)
            .with_env("RUNINATOR_INPUT", format!("{RUNTIME_DIR}/{INPUT_FILE}"))
            .with_env("RUNINATOR_OUTPUT", format!("{RUNTIME_DIR}/{OUTPUT_FILE}"))
            .with_limits(limits_for(request));

        let cancel = move || token.is_cancelled();
        let output = self
            .runner
            .run(&spec, logs, &cancel)
            .map_err(|err| map_sandbox_error(err, &request.handler))?;

        if !output.succeeded() {
            let _ = fs::remove_dir_all(&runtime_dir);
            return Err(INVOCATION_FAILED.error(format!(
                "'{}' exited with code {}: {}",
                request.handler,
                output.exit_code,
                tail(&output.stderr)
            )));
        }

        let result = read_output(&runtime_dir.join(OUTPUT_FILE))?;
        let _ = fs::remove_dir_all(&runtime_dir);
        Ok(InvocationOutcome {
            output: result,
            stdout: output.stdout,
            stderr: output.stderr,
            truncated: output.stdout_truncated || output.stderr_truncated,
            duration: output.duration,
        })
    }
}

/// the sandbox envelope an export's declared limits turn into.
///
/// the manifest supplies the numbers and the sandbox's hardened defaults supply everything the
/// manifest cannot name — no capabilities, no new privileges, an unprivileged uid — so a package
/// cannot opt out of the parts that are not its decision to make.
fn limits_for(request: &InvocationRequest) -> SandboxLimits {
    let declared = &request.limits;
    SandboxLimits {
        timeout: Duration::from_secs(request.effective_timeout_secs()),
        memory_mb: Some(declared.memory_mb.max(1)),
        cpu_millis: Some(declared.cpu_millis.max(1)),
        pids: Some(declared.pids.max(1)),
        network: declared.network,
        // a setup script installs into the image's own filesystem, so it needs a writable root; a
        // package that does not have one keeps the read-only default.
        read_only_root: request
            .runtime
            .setup_script
            .as_ref()
            .is_none_or(|script| script.trim().is_empty()),
        tmpfs_mb: Some(declared.tmp_mb.max(1)),
        ..SandboxLimits::default()
    }
}

// run the setup script, then exec the shim. `set -eu` so a failed install fails the invocation
// rather than running the handler against half-installed dependencies.
fn setup_wrapped_command(command: Vec<String>) -> Vec<String> {
    let inner = command
        .iter()
        .map(|part| format!("'{}'", part.replace('\'', "'\\''")))
        .collect::<Vec<_>>()
        .join(" ");
    vec![
        "sh".into(),
        "-c".into(),
        format!("set -eu; . {RUNTIME_DIR}/{SETUP_FILE}; exec {inner}"),
    ]
}

fn staging_dir(package_path: &Path) -> Result<std::path::PathBuf, SendableError> {
    let base = package_path
        .parent()
        .map(Path::to_path_buf)
        .unwrap_or_else(std::env::temp_dir);
    let directory = base.join(format!("invocation-{}", Uuid::new_v4().simple()));
    fs::create_dir_all(&directory).map_err(|err| {
        PACKAGE_UNREADABLE.error(format!("failed to create invocation directory: {err}"))
    })?;
    // The sandbox runs as an unprivileged uid that is different from the worker's. It only needs
    // to traverse this private, UUID-named directory to read the staged files and replace the
    // pre-created output; it must not be able to list or add unrelated files to it.
    #[cfg(unix)]
    fs::set_permissions(&directory, fs::Permissions::from_mode(0o711)).map_err(|err| {
        PACKAGE_UNREADABLE.error(format!("failed to prepare invocation directory: {err}"))
    })?;
    Ok(directory)
}

fn write_file(path: &Path, bytes: &[u8]) -> Result<(), SendableError> {
    fs::write(path, bytes).map_err(|err| {
        PACKAGE_UNREADABLE.error(format!("failed to write {}: {err}", path.display()))
    })
}

// The output file is created by the worker but replaced by the sandbox's unprivileged uid. A
// bind mount preserves the host uid, so the normal 0644 mode would make it read-only in a hardened
// container even though the mount itself is writable.
fn write_output_file(path: &Path) -> Result<(), SendableError> {
    write_file(path, b"")?;
    #[cfg(unix)]
    fs::set_permissions(path, fs::Permissions::from_mode(0o666)).map_err(|err| {
        PACKAGE_UNREADABLE.error(format!("failed to prepare {}: {err}", path.display()))
    })?;
    Ok(())
}

fn read_output(path: &Path) -> Result<Value, SendableError> {
    let text = fs::read_to_string(path)
        .map_err(|err| INVALID_OUTPUT.error(format!("function returned no output: {err}")))?;
    if text.trim().is_empty() {
        // a handler that returns nothing is legitimate; it just has no result to record.
        return Ok(Value::Null);
    }
    serde_json::from_str::<serde_json::Value>(&text)
        .map(Value::from)
        .map_err(|err| INVALID_OUTPUT.error(format!("function returned invalid json: {err}")))
}

fn map_sandbox_error(error: SandboxError, handler: &str) -> SendableError {
    match error {
        SandboxError::Cancelled => INVOCATION_CANCELED.error(format!("'{handler}' was canceled")),
        SandboxError::TimedOut(seconds) => {
            INVOCATION_TIMEOUT.error(format!("'{handler}' timed out after {seconds} seconds"))
        }
        SandboxError::RuntimeUnavailable(detail) => RUNTIME_UNAVAILABLE.error(format!(
            "this worker cannot run packaged functions: {detail}"
        )),
        other => INVOCATION_FAILED.error(other.to_string()),
    }
}

/// the last few lines of a stream, for an error message that has to fit in a status field.
fn tail(text: &str) -> String {
    let lines: Vec<&str> = text.lines().rev().take(5).collect();
    lines.into_iter().rev().collect::<Vec<_>>().join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(unix)]
    #[test]
    fn staged_runtime_keeps_the_directory_private_and_the_output_writable() {
        let parent = std::env::temp_dir().join(format!("runi-function-runtime-{}", Uuid::new_v4()));
        let package = parent.join("package");
        fs::create_dir_all(&package).unwrap();

        let runtime = staging_dir(&package).unwrap();
        let output = runtime.join(OUTPUT_FILE);
        write_output_file(&output).unwrap();

        assert_eq!(
            fs::metadata(&runtime).unwrap().permissions().mode() & 0o777,
            0o711
        );
        assert_eq!(
            fs::metadata(&output).unwrap().permissions().mode() & 0o777,
            0o666
        );

        let _ = fs::remove_dir_all(parent);
    }
}
