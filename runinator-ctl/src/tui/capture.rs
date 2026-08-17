//! taking the process's own stdout and stderr into the console's scrollback.
//!
//! the console's output pane can only scroll output it holds, and the command modules print with
//! plain `println!` — so the pipe is installed under them, at the file descriptor, rather than by
//! threading a writer through two dozen modules. a `println!` anywhere in the process (or in a
//! dependency) lands in the transcript, which is the property that makes this worth the unsafe
//! block.
//!
//! the ui itself must not go down that pipe, so `install` hands back a *duplicate of the original*
//! descriptor for the terminal backend to draw on. drawing through `io::stdout()` after this point
//! would paint the interface into the log it is displaying.
//!
//! this is a unix arrangement: on windows `println!` does not go through descriptor 1 and crossterm
//! reads the console handle the redirection would replace, so `install` reports that it is
//! unavailable and the console falls back to the plain prompt.

use std::sync::{Arc, Mutex};

use super::transcript::Transcript;
use crate::commands::{Result, err};

/// where the interface draws once the descriptors have been taken.
#[cfg(unix)]
pub(crate) type Screen = std::fs::File;
#[cfg(not(unix))]
pub(crate) type Screen = std::io::Stdout;

/// the shared log the reader thread appends to.
pub(crate) type Shared = Arc<Mutex<Transcript>>;

/// what the console holds while its output is being captured; dropping it puts the descriptors back.
pub(crate) struct Capture {
    #[cfg(unix)]
    inner: Option<unix::Redirect>,
}

impl Capture {
    /// redirect stdout and stderr into a fresh transcript.
    ///
    /// returns the log, and the terminal to draw on.
    pub(crate) fn install(limit: usize) -> Result<(Self, Screen, Shared)> {
        #[cfg(unix)]
        {
            let (redirect, screen, transcript) = unix::install(limit)?;
            Ok((
                Self {
                    inner: Some(redirect),
                },
                screen,
                transcript,
            ))
        }
        #[cfg(not(unix))]
        {
            let _ = limit;
            Err(err(
                "capturing command output needs a unix terminal on this platform",
            ))
        }
    }

    /// put the descriptors back and finish reading whatever was still in flight.
    ///
    /// separate from `Drop` because the console replays the transcript to the real terminal
    /// afterwards, and that has to happen with stdout pointing at the terminal again.
    pub(crate) fn restore(&mut self) {
        #[cfg(unix)]
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

#[cfg(unix)]
mod unix {
    use super::*;

    use std::fs::File;
    use std::io::{self, Write};
    use std::os::fd::{FromRawFd, RawFd};
    use std::thread::JoinHandle;

    /// how much the reader takes from the pipe at a time.
    const CHUNK: usize = 8 * 1024;

    pub(super) struct Redirect {
        stdout: RawFd,
        stderr: RawFd,
        reader: Option<JoinHandle<()>>,
    }

    impl Redirect {
        pub(super) fn restore(mut self) {
            // whatever is still buffered belongs in the transcript, not in the next thing written to
            // the restored terminal.
            let _ = io::stdout().flush();
            let _ = io::stderr().flush();
            unsafe {
                libc::dup2(self.stdout, libc::STDOUT_FILENO);
                libc::dup2(self.stderr, libc::STDERR_FILENO);
                libc::close(self.stdout);
                libc::close(self.stderr);
            }
            // descriptors 1 and 2 were the last write ends open, so the reader now sees end-of-file
            // and drains the rest of the pipe before it finishes.
            if let Some(reader) = self.reader.take() {
                let _ = reader.join();
            }
        }
    }

    pub(super) fn install(limit: usize) -> Result<(Redirect, Screen, Shared)> {
        // anything already buffered was written for the terminal, so it goes there before the pipe
        // takes the descriptor.
        io::stdout().flush()?;

        let (read_end, write_end) = pipe()?;
        // one duplicate for the interface to draw on and one to restore from: the interface's copy
        // is owned by a `File` and closed when the console drops it.
        let screen = duplicate(libc::STDOUT_FILENO, &[read_end, write_end])?;
        let stdout = duplicate(libc::STDOUT_FILENO, &[read_end, write_end, screen])?;
        let stderr = duplicate(libc::STDERR_FILENO, &[read_end, write_end, screen, stdout])?;

        for target in [libc::STDOUT_FILENO, libc::STDERR_FILENO] {
            if unsafe { libc::dup2(write_end, target) } < 0 {
                let failure = io::Error::last_os_error();
                unsafe {
                    libc::dup2(stdout, libc::STDOUT_FILENO);
                    libc::dup2(stderr, libc::STDERR_FILENO);
                }
                close(&[read_end, write_end, screen, stdout, stderr]);
                return Err(err(format!("cannot redirect output: {failure}")));
            }
        }
        // the descriptors are the write end now, so the original handle is no longer needed; leaving
        // it open would keep the reader waiting for an end-of-file that never comes.
        unsafe { libc::close(write_end) };

        let transcript: Shared = Arc::new(Mutex::new(Transcript::with_limit(limit)));
        let sink = Arc::clone(&transcript);
        let source = unsafe { File::from_raw_fd(read_end) };
        let reader = std::thread::Builder::new()
            .name("console-output".to_string())
            .spawn(move || pump(source, sink))?;

        Ok((
            Redirect {
                stdout,
                stderr,
                reader: Some(reader),
            },
            unsafe { File::from_raw_fd(screen) },
            transcript,
        ))
    }

    /// read the pipe until it closes, appending to the transcript as output arrives.
    ///
    /// nothing here may print: this thread is the far end of the pipe stdout now points at, and a
    /// `println!` would be feeding itself.
    ///
    /// it also must not stop early. the read end closing turns the next `println!` in the process
    /// into a broken pipe, which is a panic rather than a lost line — so a poisoned transcript is
    /// drained and discarded instead of being treated as a reason to leave.
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

    fn pipe() -> Result<(RawFd, RawFd)> {
        let mut ends: [libc::c_int; 2] = [0; 2];
        if unsafe { libc::pipe(ends.as_mut_ptr()) } != 0 {
            return Err(err(format!(
                "cannot open an output pipe: {}",
                io::Error::last_os_error()
            )));
        }
        Ok((ends[0], ends[1]))
    }

    // a duplicate of `fd`, closing what has been opened so far if it cannot be made.
    fn duplicate(fd: RawFd, opened: &[RawFd]) -> Result<RawFd> {
        let copy = unsafe { libc::dup(fd) };
        if copy < 0 {
            let failure = io::Error::last_os_error();
            close(opened);
            return Err(err(format!("cannot duplicate the terminal: {failure}")));
        }
        Ok(copy)
    }

    fn close(fds: &[RawFd]) {
        for fd in fds {
            unsafe { libc::close(*fd) };
        }
    }
}

#[cfg(all(test, unix))]
#[path = "capture_tests.rs"]
mod tests;
