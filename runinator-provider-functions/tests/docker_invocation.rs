//! end-to-end invocation against a real container runtime.
//!
//! everything else in this crate is driven through a fake runtime, which is right for the
//! request/response contract but proves nothing about whether a published package actually runs.
//! this executes a real python handler in a real container.
//!
//! it self-skips when docker is absent so an ordinary `cargo test` stays green on a machine without
//! it, and prints why — a silent skip would let this rot into a test that never runs anywhere.

use std::path::PathBuf;
use std::time::Duration;

use runinator_models::functions::{FunctionResourceLimits, FunctionRuntimeSpec};
use runinator_models::value::Value;
use runinator_plugin::cancel::CancellationToken;
use runinator_provider_functions::{DockerInvocationRuntime, InvocationRequest, InvocationRuntime};

fn docker_available() -> bool {
    if DockerInvocationRuntime::new().available() {
        return true;
    }
    println!("SKIPPING: no container runtime available");
    false
}

fn package(body: &str) -> PathBuf {
    let root = std::env::temp_dir().join(format!("runi-fn-e2e-{}", uuid::Uuid::new_v4().simple()));
    std::fs::create_dir_all(root.join("src")).unwrap();
    std::fs::write(root.join("src/images.py"), body).unwrap();
    root
}

fn request(package_path: PathBuf, input: Value, timeout: i64) -> InvocationRequest {
    InvocationRequest {
        package_path,
        handler: "src.images.resize".into(),
        runtime: FunctionRuntimeSpec::new("python3.13"),
        limits: FunctionResourceLimits {
            timeout_seconds: timeout,
            ..FunctionResourceLimits::default()
        },
        input,
        context: runinator_models::json!({ "package": "image-tools", "export": "resize" }),
        timeout_secs: 120,
    }
}

#[test]
fn a_packaged_python_function_runs_and_returns_its_output() {
    if !docker_available() {
        return;
    }
    let root = package(
        "import sys\n\
         def resize(source, width):\n\
         \x20   print('resizing', source)\n\
         \x20   print('warning', file=sys.stderr)\n\
         \x20   return {'uri': f'{source}?w={width}', 'width': width}\n",
    );
    let runtime = DockerInvocationRuntime::new();
    let outcome = runtime
        .invoke(
            &request(
                root.clone(),
                runinator_models::json!({ "source": "a.png", "width": 320 }),
                60,
            ),
            None,
            CancellationToken::new(),
        )
        .expect("invocation should succeed");

    assert_eq!(
        outcome.output.get("uri").and_then(Value::as_str),
        Some("a.png?w=320")
    );
    assert_eq!(
        outcome.output.get("width").and_then(Value::as_i64),
        Some(320)
    );
    // both streams are captured, which is what the worker turns into run chunks.
    assert!(outcome.stdout.contains("resizing a.png"));
    assert!(outcome.stderr.contains("warning"));
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn a_handler_that_raises_fails_the_invocation_with_its_traceback() {
    if !docker_available() {
        return;
    }
    let root = package("def resize(source, width):\n    raise ValueError('bad source')\n");
    let runtime = DockerInvocationRuntime::new();
    let error = runtime
        .invoke(
            &request(
                root.clone(),
                runinator_models::json!({ "source": "a.png", "width": 320 }),
                60,
            ),
            None,
            CancellationToken::new(),
        )
        .unwrap_err()
        .to_string();

    assert!(error.contains("FUNC004"), "{error}");
    // the traceback reaches the failure message, or a failing function is undebuggable.
    assert!(error.contains("bad source"), "{error}");
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn a_handler_past_its_deadline_times_out_rather_than_running_on() {
    if !docker_available() {
        return;
    }
    let root = package("import time\ndef resize(source, width):\n    time.sleep(120)\n");
    let runtime = DockerInvocationRuntime::new();
    let started = std::time::Instant::now();
    let error = runtime
        .invoke(
            &request(
                root.clone(),
                runinator_models::json!({ "source": "a.png", "width": 1 }),
                2,
            ),
            None,
            CancellationToken::new(),
        )
        .unwrap_err()
        .to_string();

    assert!(error.contains("FUNC005"), "{error}");
    // the deadline is enforced by the host, so a payload ignoring it does not get to run on.
    assert!(started.elapsed() < Duration::from_secs(60));
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn a_cancelled_invocation_is_reported_as_cancelled() {
    if !docker_available() {
        return;
    }
    let root = package("import time\ndef resize(source, width):\n    time.sleep(120)\n");
    let runtime = DockerInvocationRuntime::new();
    let token = CancellationToken::new();
    let canceller = token.clone();
    std::thread::spawn(move || {
        std::thread::sleep(Duration::from_secs(2));
        canceller.cancel();
    });

    let error = runtime
        .invoke(
            &request(
                root.clone(),
                runinator_models::json!({ "source": "a.png", "width": 1 }),
                120,
            ),
            None,
            CancellationToken::clone(&token),
        )
        .unwrap_err()
        .to_string();

    // a caller retries a timeout and does not retry a cancel, so these stay distinguishable.
    assert!(error.contains("FUNC006"), "{error}");
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn the_package_mount_is_read_only_and_the_network_is_off_by_default() {
    if !docker_available() {
        return;
    }
    let root = package(
        "import socket\n\
         def resize(source, width):\n\
         \x20   writable = True\n\
         \x20   try:\n\
         \x20       open('/package/written.txt', 'w').close()\n\
         \x20   except OSError:\n\
         \x20       writable = False\n\
         \x20   networked = True\n\
         \x20   try:\n\
         \x20       socket.create_connection(('1.1.1.1', 53), timeout=2)\n\
         \x20   except OSError:\n\
         \x20       networked = False\n\
         \x20   return {'writable': writable, 'networked': networked}\n",
    );
    let runtime = DockerInvocationRuntime::new();
    let outcome = runtime
        .invoke(
            &request(
                root.clone(),
                runinator_models::json!({ "source": "a.png", "width": 1 }),
                60,
            ),
            None,
            CancellationToken::new(),
        )
        .expect("invocation should succeed");

    // the package is shared read-only by every concurrent invocation of the same digest, so a
    // handler that could write into it would be editing code another run is executing.
    assert_eq!(
        outcome.output.get("writable").and_then(Value::as_bool),
        Some(false)
    );
    // network access is opt-in; this package did not ask for it.
    assert_eq!(
        outcome.output.get("networked").and_then(Value::as_bool),
        Some(false)
    );
    assert!(!root.join("written.txt").exists());
    let _ = std::fs::remove_dir_all(root);
}
