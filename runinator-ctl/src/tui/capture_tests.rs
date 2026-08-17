//! covers that a command's ordinary printing reaches the transcript, and that the descriptors come
//! back afterwards.
//!
//! these write to descriptor 1 directly rather than with `println!`: the test harness redirects the
//! print macros to its own per-thread sink, so a `println!` here would never reach the descriptor
//! the capture replaces. they also run one at a time — the redirection is process-wide, and two of
//! them at once would each hold half the output.
//!
//! the harness itself writes its progress to that descriptor from other threads, and while the
//! capture is installed those lines land in the transcript too. that is the feature working, so the
//! assertions are about what the log contains rather than about it holding nothing else.

use super::*;

use std::io::Write;
use std::sync::MutexGuard;
use std::sync::OnceLock;

// the descriptors are process-wide, so the tests that move them take turns.
fn exclusive() -> MutexGuard<'static, ()> {
    static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
    LOCK.get_or_init(|| Mutex::new(()))
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

fn lines(transcript: &Shared) -> Vec<String> {
    transcript
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .replay()
        .into_iter()
        .map(str::to_string)
        .collect()
}

#[test]
fn printing_lands_in_the_transcript_and_the_terminal_comes_back() {
    let _guard = exclusive();
    let saved = unsafe { libc::dup(libc::STDOUT_FILENO) };

    let (mut capture, screen, transcript) =
        Capture::install(100).expect("the descriptors can be redirected");
    {
        let mut stdout = std::io::stdout();
        let _ = writeln!(stdout, "from stdout");
        let _ = writeln!(std::io::stderr(), "from stderr");
        let _ = stdout.flush();
    }
    // restoring joins the reader, so everything written above is in the log by the time it returns.
    capture.restore();
    drop(screen);

    // both streams arrive, and they arrive in the order they were written: they share one pipe,
    // which is what keeps a command's error next to the line it belongs under.
    let captured = lines(&transcript);
    let stdout_at = captured.iter().position(|line| line == "from stdout");
    let stderr_at = captured.iter().position(|line| line == "from stderr");
    assert!(stdout_at.is_some(), "stdout was captured: {captured:?}");
    assert!(stdout_at < stderr_at, "in order: {captured:?}");

    // and the descriptor is the one the process started with.
    let restored = unsafe { libc::dup(libc::STDOUT_FILENO) };
    assert!(restored >= 0, "stdout is open again");
    unsafe {
        libc::close(restored);
        libc::close(saved);
    }
}

#[test]
fn the_interface_draws_on_the_terminal_and_not_on_the_pipe() {
    use std::os::fd::AsRawFd;

    let _guard = exclusive();
    let before = unsafe { libc::dup(libc::STDOUT_FILENO) };

    let (mut capture, screen, _transcript) =
        Capture::install(100).expect("the descriptors can be redirected");
    // the pane can only show output if the ui is not writing into it: what the interface draws on
    // has to be the terminal the process started with, while stdout has become the pipe.
    let drawn_on = identify(screen.as_raw_fd());
    let terminal = identify(before);
    let captured = identify(libc::STDOUT_FILENO);
    capture.restore();
    drop(screen);
    unsafe { libc::close(before) };

    assert_eq!(drawn_on, terminal, "the ui draws on the original stdout");
    assert_ne!(drawn_on, captured, "stdout itself has been taken");
}

// which open file a descriptor points at, as the pair that identifies one.
fn identify(fd: libc::c_int) -> (u64, u64) {
    let mut status = std::mem::MaybeUninit::<libc::stat>::uninit();
    let read = unsafe { libc::fstat(fd, status.as_mut_ptr()) };
    assert_eq!(read, 0, "descriptor {fd} can be inspected");
    let status = unsafe { status.assume_init() };
    (status.st_dev as u64, status.st_ino)
}
