//! covers the argv a spec turns into — which is where the hardening actually lives — plus the
//! output pump's bounding, and the run loop against a real command.
//!
//! the argv assertions need no container runtime on purpose: a dropped `--pids-limit` is invisible
//! until something exploits it, so it has to be checked by every ordinary test run rather than only
//! where docker happens to be installed.

use super::*;

use std::time::Duration;

use crate::spec::{Mount, SandboxLimits};

fn spec() -> ContainerSpec {
    ContainerSpec::new("python:3.13-slim", "runinator-test")
        .with_command(vec!["python".into(), "run.py".into()])
        .with_working_dir("/work")
        .with_env("RUNINATOR_OUTPUT", "/runtime/output.json")
        .with_mount(Mount::read_only("/host/pkg", "/work"))
        .with_mount(Mount::writable("/host/out", "/runtime"))
}

fn flag_value<'a>(args: &'a [String], flag: &str) -> Option<&'a str> {
    args.iter()
        .position(|arg| arg == flag)
        .and_then(|at| args.get(at + 1))
        .map(String::as_str)
}

#[test]
fn the_default_envelope_is_hardened() {
    let args = args::run_args(&spec(), "c1");

    // every one of these is load-bearing: without the network flag a compromised payload reaches
    // the cluster, without the pid cap a fork bomb takes the worker, and without the swap cap the
    // memory cap is advisory.
    assert_eq!(flag_value(&args, "--network"), Some("none"));
    assert_eq!(flag_value(&args, "--memory"), Some("512m"));
    assert_eq!(flag_value(&args, "--memory-swap"), Some("512m"));
    assert_eq!(flag_value(&args, "--cpus"), Some("1.000"));
    assert_eq!(flag_value(&args, "--pids-limit"), Some("128"));
    assert_eq!(flag_value(&args, "--user"), Some("65534:65534"));
    assert_eq!(flag_value(&args, "--cap-drop"), Some("ALL"));
    assert_eq!(
        flag_value(&args, "--security-opt"),
        Some("no-new-privileges")
    );
    assert!(args.iter().any(|arg| arg == "--read-only"));
    // a read-only root without scratch space breaks every interpreter, so the two travel together.
    assert_eq!(
        flag_value(&args, "--tmpfs"),
        Some("/tmp:rw,noexec,nosuid,size=64m")
    );
    assert!(args.iter().any(|arg| arg == "--rm"));
}

#[test]
fn mounts_env_and_command_land_in_order() {
    let args = args::run_args(&spec(), "c1");

    assert!(args.contains(&"/host/pkg:/work:ro".to_string()));
    assert!(args.contains(&"/host/out:/runtime".to_string()));
    assert_eq!(flag_value(&args, "-w"), Some("/work"));
    assert!(args.contains(&"RUNINATOR_OUTPUT=/runtime/output.json".to_string()));

    // the image separates the flags from the command, so everything after it is the payload's argv.
    let image_at = args
        .iter()
        .position(|arg| arg == "python:3.13-slim")
        .unwrap();
    assert_eq!(
        &args[image_at + 1..],
        &["python".to_string(), "run.py".to_string()]
    );
}

#[test]
fn the_compatible_envelope_emits_no_limits_but_still_bounds_output() {
    let limits = SandboxLimits::compatible(Duration::from_secs(120));
    let args = args::run_args(&spec().with_limits(limits.clone()), "c1");

    // std.code's setup_script exists to install dependencies, so it needs the network and a
    // writable root; porting it onto this runner must not silently take those away.
    assert!(!args.iter().any(|arg| arg == "--network"));
    assert!(!args.iter().any(|arg| arg == "--read-only"));
    assert!(!args.iter().any(|arg| arg == "--memory"));
    assert!(!args.iter().any(|arg| arg == "--user"));
    // output is bounded regardless, because that one is a bug fix rather than a policy.
    assert!(limits.max_output_bytes > 0);
}

#[test]
fn a_disabled_limit_emits_no_flag_at_all() {
    let limits = SandboxLimits {
        memory_mb: None,
        pids: None,
        cpu_millis: Some(2500),
        ..SandboxLimits::default()
    };
    let args = args::run_args(&spec().with_limits(limits), "c1");

    assert!(!args.iter().any(|arg| arg == "--memory"));
    assert!(!args.iter().any(|arg| arg == "--pids-limit"));
    // millis render as a fractional core count, so 2500 is two and a half cores.
    assert_eq!(flag_value(&args, "--cpus"), Some("2.500"));
}

#[test]
fn writable_roots_can_still_have_a_bounded_tmp_directory() {
    let limits = SandboxLimits {
        read_only_root: false,
        tmpfs_mb: Some(512),
        ..SandboxLimits::compatible(Duration::from_secs(30))
    };
    let args = args::run_args(&spec().with_limits(limits), "c1");

    assert!(!args.iter().any(|arg| arg == "--read-only"));
    assert_eq!(
        flag_value(&args, "--tmpfs"),
        Some("/tmp:rw,exec,nosuid,size=512m")
    );
}

#[test]
fn output_is_truncated_rather_than_allowed_to_grow() {
    let noisy = "0123456789\n".repeat(1000);
    let handle = pump::spawn(
        std::io::Cursor::new(noisy.into_bytes()),
        Stream::Stdout,
        64,
        None,
    );
    let drained = handle.join().unwrap();

    assert!(drained.truncated);
    assert!(drained.text.len() <= 64);
}

#[test]
fn the_sink_sees_every_line_even_past_the_retention_cap() {
    let sink = Arc::new(pump::RecordingSink::default());
    let handle = pump::spawn(
        std::io::Cursor::new(b"first\nsecond\nthird\n".to_vec()),
        Stream::Stderr,
        6,
        Some(sink.clone() as Arc<dyn LineSink>),
    );
    let drained = handle.join().unwrap();

    // retention and streaming are different budgets: dropping live output would hide the tail of
    // exactly the run someone is watching.
    assert_eq!(sink.lines().len(), 3);
    assert!(
        sink.lines()
            .iter()
            .all(|(stream, _)| *stream == Stream::Stderr)
    );
    assert!(drained.truncated);
}

#[test]
fn an_empty_image_is_refused_before_anything_is_spawned() {
    let runner = DockerRunner::with_binary("definitely-not-a-real-binary");
    let spec = ContainerSpec::new("  ", "runinator-test");
    let error = runner
        .run(&spec, None, &crate::never_cancelled())
        .unwrap_err();
    assert!(matches!(error, SandboxError::InvalidSpec(_)));
}

#[test]
fn a_missing_runtime_is_reported_as_unavailable_not_as_a_failure() {
    let runner = DockerRunner::with_binary("definitely-not-a-real-binary");
    assert!(!runner.available());

    let error = runner
        .run(&spec(), None, &crate::never_cancelled())
        .unwrap_err();
    // "docker is not installed" and "your code exited 1" need different answers from the caller.
    assert!(matches!(error, SandboxError::RuntimeUnavailable(_)));
}

// the two below drive a real child process rather than a container: what is under test is the
// wait/cancel/drain logic, which is backend-independent. the stand-in ignores its argv the way a
// container would ignore ours, so the loop sees exactly the shape it sees in production.
#[cfg(unix)]
fn sleeping_stand_in(name: &str) -> std::path::PathBuf {
    use std::os::unix::fs::PermissionsExt;
    let path = std::env::temp_dir().join(format!("runi-sandbox-{name}-{}", std::process::id()));
    // `rm` returns immediately, as docker's does; anything else sleeps past any deadline here.
    // without the `rm` arm the stand-in would also sleep on the abort path's force-remove, which is
    // a test artifact — but it is a fair reminder that a wedged daemon blocks that call.
    std::fs::write(
        &path,
        "#!/bin/sh\nif [ \"$1\" = rm ]; then exit 0; fi\nexec sleep 30\n",
    )
    .unwrap();
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
    path
}

#[cfg(unix)]
#[test]
fn a_run_past_its_deadline_times_out() {
    let binary = sleeping_stand_in("timeout");
    let runner = DockerRunner::with_binary(binary.to_string_lossy());
    let spec = ContainerSpec::new("ignored", "runinator-test").with_limits(SandboxLimits {
        timeout: Duration::from_secs(1),
        ..SandboxLimits::default()
    });

    let error = runner
        .run(&spec, None, &crate::never_cancelled())
        .unwrap_err();
    assert!(matches!(error, SandboxError::TimedOut(_)));
    let _ = std::fs::remove_file(binary);
}

#[cfg(unix)]
#[test]
fn a_cancelled_run_reports_cancellation_rather_than_a_timeout() {
    let binary = sleeping_stand_in("cancel");
    let runner = DockerRunner::with_binary(binary.to_string_lossy());
    let spec = ContainerSpec::new("ignored", "runinator-test").with_limits(SandboxLimits {
        timeout: Duration::from_secs(30),
        ..SandboxLimits::default()
    });

    // a caller retries a timeout and does not retry a cancel, so these must stay distinguishable.
    let error = runner.run(&spec, None, &(|| true)).unwrap_err();
    assert!(matches!(error, SandboxError::Cancelled));
    let _ = std::fs::remove_file(binary);
}
