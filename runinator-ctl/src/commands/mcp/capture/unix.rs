//! moving the standard streams with `dup2`.
//!
//! descriptors 1 and 2 are pointed at the scratch file, and `install` keeps a duplicate of each
//! original to put back. the scratch file is unlinked while both handles still hold it open, so
//! nothing is left on disk on any exit path, expected or not.

use std::fs::File;
use std::io::{self, Read, Seek, SeekFrom, Write};
use std::os::fd::{AsRawFd, FromRawFd, RawFd};

use super::{SCRATCH_LIMIT, scratch_path};
use crate::commands::{Result, err};

pub(super) struct Redirect {
    /// the scratch file, opened separately from the descriptors writing into it so it carries its
    /// own read offset.
    reader: File,
    stdout: RawFd,
    stderr: RawFd,
}

impl Redirect {
    pub(super) fn take(&mut self) -> String {
        // the command modules print through the standard streams, which buffer against a regular
        // file; an unflushed line is output that has not happened yet.
        let _ = io::stdout().flush();
        let _ = io::stderr().flush();

        let mut bytes = Vec::new();
        let text = match self.reader.read_to_end(&mut bytes) {
            Ok(_) => String::from_utf8_lossy(&bytes).into_owned(),
            Err(_) => String::new(),
        };
        self.rewind_when_large();
        text
    }

    pub(super) fn restore(self) {
        let _ = io::stdout().flush();
        let _ = io::stderr().flush();
        unsafe {
            libc::dup2(self.stdout, libc::STDOUT_FILENO);
            libc::dup2(self.stderr, libc::STDERR_FILENO);
            libc::close(self.stdout);
            libc::close(self.stderr);
        }
    }

    // the writing descriptors and the reader each carry their own offset into the file, so
    // truncating means seeking all three back. doing it only past the limit keeps the ordinary path
    // to a single read.
    fn rewind_when_large(&mut self) {
        let large = self
            .reader
            .stream_position()
            .is_ok_and(|position| position > SCRATCH_LIMIT);
        if !large || self.reader.set_len(0).is_err() {
            return;
        }
        let _ = self.reader.seek(SeekFrom::Start(0));
        unsafe {
            libc::lseek(libc::STDOUT_FILENO, 0, libc::SEEK_SET);
            libc::lseek(libc::STDERR_FILENO, 0, libc::SEEK_SET);
        }
    }
}

pub(super) fn install() -> Result<(Redirect, File)> {
    // anything already buffered was written for the terminal, so it goes there before the
    // descriptor is taken.
    io::stdout().flush()?;

    let path = scratch_path();
    let sink = File::create(&path)
        .map_err(|failure| err(format!("cannot open a scratch file for output: {failure}")))?;
    let reader = File::open(&path)
        .map_err(|failure| err(format!("cannot read back captured output: {failure}")))?;
    // both handles keep the file open, so unlinking it now means nothing is left behind on any exit
    // path, expected or not.
    let _ = std::fs::remove_file(&path);

    let screen = duplicate(libc::STDOUT_FILENO, &[])?;
    let stdout = duplicate(libc::STDOUT_FILENO, &[screen])?;
    let stderr = duplicate(libc::STDERR_FILENO, &[screen, stdout])?;

    for target in [libc::STDOUT_FILENO, libc::STDERR_FILENO] {
        if unsafe { libc::dup2(sink.as_raw_fd(), target) } < 0 {
            let failure = io::Error::last_os_error();
            unsafe {
                libc::dup2(stdout, libc::STDOUT_FILENO);
                libc::dup2(stderr, libc::STDERR_FILENO);
            }
            close(&[screen, stdout, stderr]);
            return Err(err(format!("cannot redirect output: {failure}")));
        }
    }

    Ok((
        Redirect {
            reader,
            stdout,
            stderr,
        },
        unsafe { File::from_raw_fd(screen) },
    ))
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
