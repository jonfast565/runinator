#[allow(unused_imports)]
use super::*;

pub struct DockerInvocationRuntime {
    pub(super) runner: DockerRunner,
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
