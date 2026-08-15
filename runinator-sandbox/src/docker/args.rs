//! building the `docker run` argv.
//!
//! kept pure and separate from the process handling so the security-relevant part — which flags a
//! given [`SandboxLimits`] actually turns into — is assertable without docker installed. a hardening
//! flag that silently stopped being emitted is exactly the kind of regression that is invisible
//! until it matters.

use crate::spec::{ContainerSpec, SandboxLimits};

/// where the writable tmpfs is mounted when the root filesystem is read-only.
pub const TMPFS_TARGET: &str = "/tmp";

/// the full argument list for `docker`, starting at `run`.
pub fn run_args(spec: &ContainerSpec, container_name: &str) -> Vec<String> {
    let mut args: Vec<String> = vec![
        "run".into(),
        // --rm covers the ordinary exit; a kill path still force-removes, because a container the
        // daemon is still tearing down is one the next run can collide with by name.
        "--rm".into(),
        "-i".into(),
        "--name".into(),
        container_name.into(),
    ];
    args.extend(limit_args(&spec.limits));

    for mount in &spec.mounts {
        args.push("-v".into());
        let suffix = if mount.read_only { ":ro" } else { "" };
        args.push(format!(
            "{}:{}{suffix}",
            mount.source.display(),
            mount.target
        ));
    }
    if let Some(working_dir) = &spec.working_dir {
        args.push("-w".into());
        args.push(working_dir.clone());
    }
    for (key, value) in &spec.env {
        args.push("-e".into());
        args.push(format!("{key}={value}"));
    }

    args.push(spec.image.clone());
    args.extend(spec.command.iter().cloned());
    args
}

/// the flags a limit set turns into.
fn limit_args(limits: &SandboxLimits) -> Vec<String> {
    let mut args = Vec::new();
    if !limits.network {
        args.push("--network".into());
        args.push("none".into());
    }
    if let Some(memory_mb) = limits.memory_mb {
        args.push("--memory".into());
        args.push(format!("{memory_mb}m"));
        // without a matching swap cap the memory cap is advisory: the payload spills into swap
        // instead of being killed. equal values mean "this much total, no swap".
        args.push("--memory-swap".into());
        args.push(format!("{memory_mb}m"));
    }
    if let Some(cpu_millis) = limits.cpu_millis {
        args.push("--cpus".into());
        args.push(format!("{:.3}", cpu_millis as f64 / 1000.0));
    }
    if let Some(pids) = limits.pids {
        args.push("--pids-limit".into());
        args.push(pids.to_string());
    }
    if let Some(user) = &limits.user {
        args.push("--user".into());
        args.push(user.clone());
    }
    if limits.drop_capabilities {
        args.push("--cap-drop".into());
        args.push("ALL".into());
    }
    if limits.no_new_privileges {
        args.push("--security-opt".into());
        args.push("no-new-privileges".into());
    }
    if limits.read_only_root {
        args.push("--read-only".into());
        // a read-only root with no scratch space breaks nearly every interpreter, so the tmpfs is
        // part of the same decision rather than an independent option.
        let tmpfs_mb = limits.tmpfs_mb.unwrap_or(64);
        args.push("--tmpfs".into());
        args.push(format!("{TMPFS_TARGET}:rw,noexec,nosuid,size={tmpfs_mb}m"));
    }
    args
}

/// the arguments that force-remove a container by name.
pub fn remove_args(container_name: &str) -> Vec<String> {
    vec!["rm".into(), "-f".into(), container_name.into()]
}
