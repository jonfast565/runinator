//! covers taking the process's stdout away from the protocol channel and giving it back.
//!
//! these run on every platform the capture supports, against whichever `capture/` module was
//! compiled in — the point of asserting behaviour rather than syscalls is that `dup2` and
//! `SetStdHandle` have to produce the same observable result.
//!
//! two things about how they are written. they move a process-wide standard stream, so they hold a
//! lock against each other: two redirections installed at once would each restore the other's.
//! and they write through `io::stdout()` rather than with `println!`, because the test harness
//! hooks the print macros into a per-test buffer that never reaches the standard stream at all —
//! the stream is what the server redirects, and the stream is what these have to exercise.

use super::*;

use std::io::Write;
use std::sync::{Mutex, MutexGuard};

fn exclusive() -> MutexGuard<'static, ()> {
    static LOCK: Mutex<()> = Mutex::new(());
    LOCK.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
}

fn to_stdout(text: &str) {
    let mut stdout = std::io::stdout();
    let _ = writeln!(stdout, "{text}");
}

// the reason this exists at all: a table written into the middle of a json-rpc frame would
// desynchronise the client, so command output has to stop reaching the protocol channel.
#[test]
fn printing_lands_in_the_capture() {
    let _guard = exclusive();
    let (mut capture, _screen) = OutputCapture::install().expect("capture installs");

    to_stdout("a table nobody should see");
    let captured = capture.take();
    capture.restore();

    assert!(
        captured.contains("a table nobody should see"),
        "captured {captured:?}"
    );
}

// a tool result has to say exactly what this command printed, with nothing of the last one in it.
#[test]
fn each_take_returns_only_what_was_written_since_the_last_one() {
    let _guard = exclusive();
    let (mut capture, _screen) = OutputCapture::install().expect("capture installs");

    to_stdout("mcp-capture-alpha");
    let first = capture.take();
    to_stdout("mcp-capture-beta");
    let second = capture.take();
    capture.restore();

    assert!(
        first.contains("mcp-capture-alpha") && !first.contains("mcp-capture-beta"),
        "{first:?}"
    );
    assert!(
        second.contains("mcp-capture-beta") && !second.contains("mcp-capture-alpha"),
        "{second:?}"
    );
}

// stderr is redirected too: a warning printed by a dependency would otherwise interleave with the
// frames.
#[test]
fn stderr_is_captured_as_well() {
    let _guard = exclusive();
    let (mut capture, _screen) = OutputCapture::install().expect("capture installs");

    let _ = writeln!(std::io::stderr(), "mcp-capture-warning");
    let captured = capture.take();
    capture.restore();

    assert!(
        captured.contains("mcp-capture-warning"),
        "captured {captured:?}"
    );
}

// output is consumed, not accumulated: a second command must not be told what the first one said.
//
// this is asserted as "the marker is gone" rather than as "the capture is empty" on purpose — the
// test harness writes its own progress lines to descriptor 1 while other tests run in parallel, so
// an empty capture is not something this process can arrange.
#[test]
fn a_take_does_not_repeat_what_the_last_one_returned() {
    let _guard = exclusive();
    let (mut capture, _screen) = OutputCapture::install().expect("capture installs");

    to_stdout("mcp-capture-marker-once");
    let first = capture.take();
    let second = capture.take();
    capture.restore();

    assert!(first.contains("mcp-capture-marker-once"), "{first:?}");
    assert!(!second.contains("mcp-capture-marker-once"), "{second:?}");
}

// the descriptors go back before the process ends, so anything printed afterwards reaches the
// terminal rather than the scratch file.
#[test]
fn restoring_is_idempotent_and_survives_the_drop() {
    let _guard = exclusive();
    let (mut capture, _screen) = OutputCapture::install().expect("capture installs");
    capture.restore();
    // a second restore has nothing to put back, and must not close a descriptor twice.
    capture.restore();
    assert_eq!(capture.take(), "");
    drop(capture);
}

// however the server ends, it leaves no scratch file behind: unix unlinks it while both handles
// still hold it open, and windows opens it delete-on-close.
//
// asserted after the capture is dropped rather than during it, because the two platforms differ on
// exactly that point — the unix file is already gone the moment it is opened, the windows one is
// visible until the last handle closes. what they agree on is the state afterwards, which is the
// property worth holding.
#[test]
fn the_scratch_file_is_not_left_on_disk() {
    let _guard = exclusive();
    let before = scratch_files();

    let (mut capture, screen) = OutputCapture::install().expect("capture installs");
    to_stdout("something");
    let _ = capture.take();
    capture.restore();
    drop(capture);
    drop(screen);

    assert_eq!(scratch_files(), before, "a scratch file was left behind");
}

fn scratch_files() -> usize {
    let Ok(entries) = std::fs::read_dir(std::env::temp_dir()) else {
        return 0;
    };
    entries
        .filter_map(std::result::Result::ok)
        .filter(|entry| {
            entry
                .file_name()
                .to_string_lossy()
                .starts_with("runinator-mcp-")
        })
        .count()
}
