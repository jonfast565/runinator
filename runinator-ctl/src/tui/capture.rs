//! taking the process's own stdout and stderr into the console's scrollback.
//!
//! the console's output pane can only scroll output it holds, and the command modules print with
//! plain `println!` — so the pipe is installed under them, at the standard stream, rather than by
//! threading a writer through two dozen modules. a `println!` anywhere in the process (or in a
//! dependency) lands in the transcript, which is the property that makes this worth the unsafe
//! block.
//!
//! the UI itself must not go down that pipe, so `install` hands back a separate handle on the real
//! terminal for the backend to draw on. drawing through `io::stdout()` after this point would paint
//! the interface into the log it is displaying.
//!
//! moving a standard stream is the one per-platform part, and it is the whole of `capture/unix.rs`
//! and `capture/windows.rs`: `dup2` on a descriptor there, `SetStdHandle` on a std handle here.
//! the pipe reader below is shared, because a pipe reads the same either way.
//!
//! crossterm is what makes the windows half possible, and it is worth saying why: it reaches the
//! terminal through `CONOUT$` and `CONIN$`, opened by name with `CreateFileW`, and never through
//! `GetStdHandle`. so the redirection cannot disturb the size query, raw mode, the alternate
//! screen, or the event source — the two are looking at different things by construction.

use std::fs::File;
use std::io;
use std::sync::{Arc, Mutex};

use super::transcript::Transcript;
use crate::commands::Result;

#[cfg(unix)]
#[path = "capture/unix.rs"]
mod platform;

#[cfg(windows)]
#[path = "capture/windows.rs"]
mod platform;

#[cfg(not(any(unix, windows)))]
#[path = "capture/unsupported.rs"]
mod platform;

/// how much the reader takes from the pipe at a time.
const CHUNK: usize = 8 * 1024;

/// where the interface draws once the standard streams have been taken.
///
/// a file either way: a duplicate of the original stdout on unix, a fresh handle on `CONOUT$` on
/// windows. what matters to the caller is that it is not `io::stdout()`.
pub(crate) type Screen = File;

/// the shared log the reader thread appends to.
pub(crate) type Shared = Arc<Mutex<Transcript>>;

/// what the console holds while its output is being captured; dropping it puts the streams back.
pub(crate) struct Capture {
    inner: Option<platform::Redirect>,
}

impl Capture {
    /// redirect stdout and stderr into a fresh transcript.
    ///
    /// returns the log, and the terminal to draw on.
    pub(crate) fn install(limit: usize) -> Result<(Self, Screen, Shared)> {
        let (redirect, screen, transcript) = platform::install(limit)?;
        Ok((
            Self {
                inner: Some(redirect),
            },
            screen,
            transcript,
        ))
    }

    /// put the streams back and finish reading whatever was still in flight.
    ///
    /// separate from `Drop` because the console replays the transcript to the real terminal
    /// afterwards, and that has to happen with stdout pointing at the terminal again.
    pub(crate) fn restore(&mut self) {
        if let Some(redirect) = self.inner.take() {
            redirect.restore();
        }
    }
}

impl Drop for Capture {
    fn drop(&mut self) {
        self.restore();
    }
}

/// read the pipe until it closes, appending to the transcript as output arrives.
///
/// nothing here may print: this thread is the far end of the pipe stdout now points at, and a
/// `println!` would be feeding itself.
///
/// it also must not stop early. on unix the read end closing turns the next `println!` in the
/// process into a broken pipe, which is a panic rather than a lost line — so a poisoned transcript
/// is drained and discarded instead of being treated as a reason to leave.
///
/// the two platforms spell the end differently: a closed unix pipe reads zero bytes, a closed
/// windows one fails with `ERROR_BROKEN_PIPE`. both mean the same thing here, which is why any
/// error that is not an interruption ends the loop rather than being reported.
fn pump(mut source: File, sink: Shared) {
    use std::io::Read;

    let mut buffer = [0u8; CHUNK];
    // a chunk can end mid-character, so the undecodable tail waits for the next read.
    let mut tail: Vec<u8> = Vec::new();
    loop {
        let read = match source.read(&mut buffer) {
            Ok(0) => break,
            Ok(read) => read,
            Err(error) if error.kind() == io::ErrorKind::Interrupted => continue,
            Err(_) => break,
        };
        tail.extend_from_slice(&buffer[..read]);
        let text = match std::str::from_utf8(&tail) {
            Ok(text) => {
                let owned = text.to_string();
                tail.clear();
                owned
            }
            Err(error) => {
                let at = error.valid_up_to();
                let owned = String::from_utf8_lossy(&tail[..at]).into_owned();
                tail.drain(..at);
                // a tail that cannot be a partial character is broken, not incomplete.
                if tail.len() > 4 {
                    tail.clear();
                }
                owned
            }
        };
        if text.is_empty() {
            continue;
        }
        sink.lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .write(&text);
    }
}

/// start the reader thread over a pipe's read end.
fn spawn_reader(source: File, limit: usize) -> Result<(std::thread::JoinHandle<()>, Shared)> {
    let transcript: Shared = Arc::new(Mutex::new(Transcript::with_limit(limit)));
    let sink = Arc::clone(&transcript);
    let reader = std::thread::Builder::new()
        .name("console-output".to_string())
        .spawn(move || pump(source, sink))?;
    Ok((reader, transcript))
}

#[cfg(test)]
#[path = "capture_tests.rs"]
mod tests;
