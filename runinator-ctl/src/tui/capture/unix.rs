//! moving the standard streams with `dup2`.
//!
//! descriptors 1 and 2 are pointed at the pipe's write end, and `install` keeps a duplicate of each
//! original: one to restore from, and one for the interface to draw on.

use std::fs::File;
use std::io::{self, Write};
use std::os::fd::{FromRawFd, RawFd};
use std::thread::JoinHandle;

use super::{Screen, Shared, spawn_reader};
use crate::commands::{Result, err};

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
        // descriptors 1 and 2 were the last write ends open, so the reader now sees end-of-file and
        // drains the rest of the pipe before it finishes.
        if let Some(reader) = self.reader.take() {
            let _ = reader.join();
        }
    }
}

pub(super) fn install(limit: usize) -> Result<(Redirect, Screen, Shared)> {
    // anything already buffered was written for the terminal, so it goes there before the pipe takes
    // the descriptor.
    io::stdout().flush()?;

    let (read_end, write_end) = pipe()?;
    // one duplicate for the interface to draw on and one to restore from: the interface's copy is
    // owned by a `File` and closed when the console drops it.
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
    // the descriptors are the write end now, so the original handle is no longer needed; leaving it
    // open would keep the reader waiting for an end-of-file that never comes.
    unsafe { libc::close(write_end) };

    let source = unsafe { File::from_raw_fd(read_end) };
    let (reader, transcript) = spawn_reader(source, limit)?;

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
