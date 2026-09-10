//! process completion, cancellation, and blocked-input deadlines.
use super::*;

#[test]
fn canceled_request_does_not_spawn() {
    let token = CancellationToken::new();
    token.cancel();
    let result = NativeProcessRunner.run(ProcessRequest {
        command: &mut Command::new("missing-command"),
        input: None,
        timeout: Duration::from_secs(1),
        cancellation: &token,
        sink: None,
    });
    assert!(matches!(result, Err(ProcessFailure::Canceled)));
}

#[cfg(unix)]
#[test]
fn captures_both_streams_and_nonzero_exit() {
    let mut command = Command::new("sh");
    command.args(["-c", "cat; echo problem >&2; exit 7"]);
    let result = NativeProcessRunner
        .run(ProcessRequest {
            command: &mut command,
            input: Some(b"input\n".to_vec()),
            timeout: Duration::from_secs(2),
            cancellation: &CancellationToken::new(),
            sink: None,
        })
        .unwrap();
    assert_eq!(result.status.code(), Some(7));
    assert_eq!(result.output.stdout, "input\n");
    assert_eq!(result.output.stderr, "problem\n");
}

#[cfg(unix)]
#[test]
fn deadline_interrupts_blocked_stdin() {
    let started = Instant::now();
    let mut command = Command::new("sleep");
    command.arg("10");
    let result = NativeProcessRunner.run(ProcessRequest {
        command: &mut command,
        input: Some(vec![b'x'; 1024 * 1024]),
        timeout: Duration::from_millis(50),
        cancellation: &CancellationToken::new(),
        sink: None,
    });
    assert!(matches!(result, Err(ProcessFailure::TimedOut)));
    assert!(started.elapsed() < Duration::from_secs(2));
}
