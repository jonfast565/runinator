//! subprocess helpers, mirroring build.ps1's `Invoke-ExternalCommand`/`Invoke-Kubectl` family but
//! without a shell dependency: every call goes straight through `std::process::Command`.

use std::io::Write;
use std::path::Path;
use std::process::{Command, Stdio};

use anyhow::{Context, Result, bail};

fn display_command(program: &str, args: &[&str]) -> String {
    if args.is_empty() {
        program.to_string()
    } else {
        format!("{program} {}", args.join(" "))
    }
}

/// runs `program args...` in `cwd` with inherited stdio, failing if the exit code is non-zero.
pub fn run(program: &str, args: &[&str], cwd: &Path) -> Result<()> {
    run_with_env(program, args, cwd, &[])
}

/// like [`run`], additionally setting `envs` on the child process.
pub fn run_with_env(program: &str, args: &[&str], cwd: &Path, envs: &[(&str, &str)]) -> Result<()> {
    println!(">> {}", display_command(program, args));
    let status = Command::new(program)
        .args(args)
        .current_dir(cwd)
        .envs(envs.iter().copied())
        .status()
        .with_context(|| format!("failed to spawn '{}'", display_command(program, args)))?;

    if !status.success() {
        bail!(
            "command '{}' failed with {status}",
            display_command(program, args)
        );
    }
    Ok(())
}

/// runs `program args...` in `cwd`, capturing and returning stdout. fails on a non-zero exit code.
pub fn capture(program: &str, args: &[&str], cwd: &Path) -> Result<String> {
    let output = Command::new(program)
        .args(args)
        .current_dir(cwd)
        .output()
        .with_context(|| format!("failed to spawn '{}'", display_command(program, args)))?;

    if !output.status.success() {
        bail!(
            "command '{}' failed with {}: {}",
            display_command(program, args),
            output.status,
            String::from_utf8_lossy(&output.stderr)
        );
    }
    Ok(String::from_utf8_lossy(&output.stdout).into_owned())
}

/// runs `program args...` in `cwd`, capturing stdout but ignoring a non-zero exit code (mirrors
/// call sites that only care whether any matching output came back, e.g. `kubectl get ... || true`).
pub fn capture_allow_failure(program: &str, args: &[&str], cwd: &Path) -> String {
    Command::new(program)
        .args(args)
        .current_dir(cwd)
        .output()
        .map(|output| String::from_utf8_lossy(&output.stdout).into_owned())
        .unwrap_or_default()
}

/// runs `program args...` in `cwd`, writing `stdin_data` to the child's stdin. used for
/// `kubectl apply/delete -f -` against a filtered/rendered manifest instead of a file path.
pub fn run_with_stdin(program: &str, args: &[&str], cwd: &Path, stdin_data: &str) -> Result<()> {
    println!(">> {}  (piped via stdin)", display_command(program, args));
    let mut child = Command::new(program)
        .args(args)
        .current_dir(cwd)
        .stdin(Stdio::piped())
        .spawn()
        .with_context(|| format!("failed to spawn '{}'", display_command(program, args)))?;

    child
        .stdin
        .take()
        .expect("child stdin was piped")
        .write_all(stdin_data.as_bytes())?;

    let status = child.wait()?;
    if !status.success() {
        bail!(
            "command '{}' failed with {status}",
            display_command(program, args)
        );
    }
    Ok(())
}

/// runs `f`, logging (but not propagating) any error. mirrors build.ps1's best-effort
/// `try { ... } catch { Write-Warning ... }` cleanup/diagnostic steps.
pub fn warn_on_err(context: &str, f: impl FnOnce() -> Result<()>) {
    if let Err(err) = f() {
        eprintln!("warning: {context}: {err:#}");
    }
}

/// true if `name` resolves on PATH.
pub fn tool_available(name: &str) -> bool {
    which(name).is_some()
}

fn which(name: &str) -> Option<std::path::PathBuf> {
    let path_var = std::env::var_os("PATH")?;
    let exe_suffix = if cfg!(target_os = "windows") {
        ".exe"
    } else {
        ""
    };
    std::env::split_paths(&path_var).find_map(|dir| {
        let candidate = dir.join(format!("{name}{exe_suffix}"));
        candidate.is_file().then_some(candidate)
    })
}

pub fn require_tool(name: &str) -> Result<()> {
    if tool_available(name) {
        Ok(())
    } else {
        bail!("required tool '{name}' was not found on PATH")
    }
}
