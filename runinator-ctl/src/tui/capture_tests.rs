//! covers that a command's ordinary printing reaches the transcript, that the interface's own
//! drawing does not, and that the streams come back afterwards.
//!
//! these write to `io::stdout()` directly rather than with `println!`: the test harness redirects
//! the print macros to its own per-thread sink, so a `println!` here would never reach the stream
//! the capture replaces. they also run one at a time — the redirection is process-wide, and two of
//! them at once would each hold half the output.
//!
//! the harness itself writes its progress to that stream from other threads, and while the capture
//! is installed those lines land in the transcript too. that is the feature working, so the
//! assertions are about what the log contains rather than about it holding nothing else — and the
//! markers are distinctive so that nothing the harness prints can satisfy or break one.
//!
//! all but the last are platform-neutral on purpose. `dup2` and `SetStdHandle` are different
//! syscalls reaching for the same result, and the result is the thing worth pinning.

use super::*;

use std::io::Write;
use std::sync::MutexGuard;
use std::sync::OnceLock;

// the streams are process-wide, so the tests that move them take turns.
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

fn holding(captured: &[String], marker: &str) -> bool {
    captured.iter().any(|line| line.contains(marker))
}

#[test]
fn printing_lands_in_the_transcript() {
    let _guard = exclusive();

    let (mut capture, screen, transcript) =
        Capture::install(500).expect("the streams can be redirected");
    {
        let mut stdout = std::io::stdout();
        let _ = writeln!(stdout, "console-capture-from-stdout");
        let _ = writeln!(std::io::stderr(), "console-capture-from-stderr");
        let _ = stdout.flush();
    }
    // restoring joins the reader, so everything written above is in the log by the time it returns.
    capture.restore();
    drop(screen);

    // both streams arrive, and they arrive in the order they were written: they share one pipe,
    // which is what keeps a command's error next to the line it belongs under.
    let captured = lines(&transcript);
    let stdout_at = captured
        .iter()
        .position(|line| line.contains("console-capture-from-stdout"));
    let stderr_at = captured
        .iter()
        .position(|line| line.contains("console-capture-from-stderr"));
    assert!(stdout_at.is_some(), "stdout was captured: {captured:?}");
    assert!(stderr_at.is_some(), "stderr was captured: {captured:?}");
    assert!(stdout_at < stderr_at, "in order: {captured:?}");
}

// the pane can only show output if the interface is not writing into it. this is the whole reason
// `install` hands back a separate handle rather than letting the UI use `io::stdout()`.
#[test]
fn the_interface_does_not_draw_into_its_own_log() {
    let _guard = exclusive();

    let (mut capture, mut screen, transcript) =
        Capture::install(500).expect("the streams can be redirected");
    let _ = writeln!(screen, "console-capture-drawn-frame");
    let _ = screen.flush();
    // something down the pipe as well, so a transcript that stayed empty for an unrelated reason
    // cannot pass this by holding nothing at all.
    let _ = writeln!(std::io::stdout(), "console-capture-printed-line");
    capture.restore();
    drop(screen);

    let captured = lines(&transcript);
    assert!(
        holding(&captured, "console-capture-printed-line"),
        "the pipe is live: {captured:?}"
    );
    assert!(
        !holding(&captured, "console-capture-drawn-frame"),
        "the interface painted into the pane it draws: {captured:?}"
    );
}

// a restore that only half worked would leave the next console with a stream pointing into a pipe
// nobody drains, which is a hang rather than a failure — so the second cycle is the assertion.
#[test]
fn the_streams_come_back_well_enough_to_capture_again() {
    let _guard = exclusive();

    let (mut first, first_screen, first_log) =
        Capture::install(500).expect("the streams can be redirected");
    let _ = writeln!(std::io::stdout(), "console-capture-cycle-one");
    first.restore();
    drop(first_screen);

    let (mut second, second_screen, second_log) =
        Capture::install(500).expect("the streams can be redirected a second time");
    let _ = writeln!(std::io::stdout(), "console-capture-cycle-two");
    second.restore();
    drop(second_screen);

    let one = lines(&first_log);
    let two = lines(&second_log);
    assert!(holding(&one, "console-capture-cycle-one"), "{one:?}");
    assert!(holding(&two, "console-capture-cycle-two"), "{two:?}");
    // each cycle got its own pipe: the first log stopped receiving when it was restored.
    assert!(
        !holding(&one, "console-capture-cycle-two"),
        "the first capture was still live: {one:?}"
    );
}

// the same claim as `the_interface_does_not_draw_into_its_own_log`, but as identity rather than
// On Unix, the UI draws on the same file stdout used at startup. Windows reaches
// the console by name (`CONOUT$`) instead, where there is no such thing to compare.
#[cfg(unix)]
#[test]
fn the_interface_draws_on_the_original_stdout() {
    use std::os::fd::AsRawFd;

    let _guard = exclusive();
    let before = unsafe { libc::dup(libc::STDOUT_FILENO) };

    let (mut capture, screen, _transcript) =
        Capture::install(100).expect("the descriptors can be redirected");
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
#[cfg(unix)]
fn identify(fd: libc::c_int) -> (u64, u64) {
    let mut status = std::mem::MaybeUninit::<libc::stat>::uninit();
    let read = unsafe { libc::fstat(fd, status.as_mut_ptr()) };
    assert_eq!(read, 0, "descriptor {fd} can be inspected");
    let status = unsafe { status.assume_init() };
    (status.st_dev as u64, status.st_ino)
}

// the console prints its greeting *after* the interface is up, so that the banner is the first
// thing in the output pane rather than the last thing on the screen the console took over. this is
// that arrangement without the console: what a command prints first is what the pane shows first.
//
// asserted as relative order rather than as "line zero" because the test harness writes its own
// progress to this stream from other threads while the capture is installed.
#[test]
fn what_is_printed_first_is_what_the_pane_shows_first() {
    let _guard = exclusive();

    let (mut capture, screen, transcript) =
        Capture::install(500).expect("the streams can be redirected");
    {
        let mut stdout = std::io::stdout();
        let _ = writeln!(stdout, "{}", crate::banner::text());
        let _ = writeln!(stdout, "console-capture-later-work");
        let _ = stdout.flush();
    }
    capture.restore();
    drop(screen);

    let captured = lines(&transcript);
    let crest = crate::banner::text()
        .lines()
        .find(|line| line.contains("_ \\"))
        .expect("the banner has a figlet row");
    let banner_at = captured.iter().position(|line| line.contains(crest));
    let work_at = captured
        .iter()
        .position(|line| line.contains("console-capture-later-work"));
    assert!(
        banner_at.is_some(),
        "the banner reached the pane: {captured:?}"
    );
    assert!(
        banner_at < work_at,
        "the banner is above the work: {captured:?}"
    );
}
