//! taking the process's own stdout and stderr away from the protocol channel.
//!
//! the mcp server answers on stdout, and every command module prints with plain `println!` — so a
//! single `workflows list` would write a table into the middle of a json-rpc frame and desynchronise
//! the client. the redirection is therefore installed *under* the command modules, at the file
//! descriptor, and `install` hands back a duplicate of the original stdout for the protocol to
//! answer on. this is the same arrangement the console uses, for the same reason.
//!
//! it is not `tui::capture`, and the difference is the sync point. the console streams into a
//! line-limited scrollback on a reader thread and never has to say "this command is finished"; a
//! tool result does, exactly, with nothing of the next command's output in it and nothing of this
//! one's missing. so the descriptors are pointed at an unlinked scratch file rather than at a pipe:
//! a flush is the sync point, the read is ordinary file i/o, and there is no reader thread to race.
//!
//! this is a unix arrangement, for the reason documented on `tui::capture`: on windows `println!`
//! does not travel through descriptor 1, so there is nothing to redirect.

use std::fs::File;

use crate::commands::{Result, err};

/// what the server holds while command output is being captured; dropping it puts the descriptors
/// back.
pub(crate) struct OutputCapture {
    #[cfg(unix)]
    inner: Option<unix::Redirect>,
}

impl OutputCapture {
    /// redirect stdout and stderr into a scratch file, returning the real stdout to answer on.
    pub(crate) fn install() -> Result<(Self, File)> {
        #[cfg(unix)]
        {
            let (redirect, screen) = unix::install()?;
            Ok((
                Self {
                    inner: Some(redirect),
                },
                screen,
            ))
        }
        #[cfg(not(unix))]
        {
            Err(err(
                "the mcp server captures command output, which needs a unix process on this platform",
            ))
        }
    }

    /// everything written since the last call.
    pub(crate) fn take(&mut self) -> String {
        #[cfg(unix)]
        {
            match self.inner.as_mut() {
                Some(redirect) => redirect.take(),
                None => String::new(),
            }
        }
        #[cfg(not(unix))]
        String::new()
    }

    /// put the descriptors back.
    pub(crate) fn restore(&mut self) {
        #[cfg(unix)]
        if let Some(redirect) = self.inner.take() {
            redirect.restore();
        }
    }
}

impl Drop for OutputCapture {
    fn drop(&mut self) {
        self.restore();
    }
}

#[cfg(unix)]
mod unix {
    use super::*;

    use std::io::{self, Read, Seek, SeekFrom, Write};
    use std::os::fd::{AsRawFd, FromRawFd, RawFd};
    use std::path::PathBuf;

    /// how much output is retained before the scratch file is rewound.
    ///
    /// output is read and discarded per command, so this only bounds what a single runaway command
    /// can leave behind between two reads.
    const SCRATCH_LIMIT: u64 = 64 * 1024 * 1024;

    pub(super) struct Redirect {
        /// the scratch file, opened separately from the descriptors writing into it so it carries
        /// its own read offset.
        reader: File,
        stdout: RawFd,
        stderr: RawFd,
    }

    impl Redirect {
        pub(super) fn take(&mut self) -> String {
            // the command modules print through the standard streams, which buffer against a
            // regular file; an unflushed line is output that has not happened yet.
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
        // truncating means seeking all three back. doing it only past the limit keeps the ordinary
        // path to a single read.
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
        // both handles keep the file open, so unlinking it now means nothing is left behind on any
        // exit path, expected or not.
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

    fn scratch_path() -> PathBuf {
        let unique = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|elapsed| elapsed.as_nanos())
            .unwrap_or_default();
        std::env::temp_dir().join(format!("runinator-mcp-{}-{unique}.out", std::process::id()))
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
