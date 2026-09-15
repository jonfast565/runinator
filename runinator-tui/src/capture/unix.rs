//! Unix implementation of the shared dashboard stream capture.

use super::{Dashboard, Reader, Screen, spawn_reader};
use std::fs::File;
use std::io::{self, Write};
use std::os::fd::{FromRawFd, RawFd};
use std::sync::Arc;

pub(super) struct Redirect {
    stdout: RawFd,
    stderr: RawFd,
    reader: Option<Reader>,
}

impl Redirect {
    pub(super) fn restore(mut self) {
        // Flush before restoring: bytes buffered by Rust still belong to the dashboard, not the
        // shell that becomes visible when the alternate screen closes.
        let _ = io::stdout().flush();
        let _ = io::stderr().flush();
        unsafe {
            libc::dup2(self.stdout, libc::STDOUT_FILENO);
            libc::dup2(self.stderr, libc::STDERR_FILENO);
            libc::close(self.stdout);
            libc::close(self.stderr);
        }
        if let Some(reader) = self.reader.take() {
            reader.finish();
        }
    }
}

pub(super) fn install(dashboard: Arc<Dashboard>) -> io::Result<(Redirect, Screen)> {
    io::stdout().flush()?;
    io::stderr().flush()?;

    let (read_end, write_end) = pipe()?;
    let screen = duplicate(libc::STDOUT_FILENO, &[read_end, write_end])?;
    let stdout = duplicate(libc::STDOUT_FILENO, &[read_end, write_end, screen])?;
    let stderr = duplicate(libc::STDERR_FILENO, &[read_end, write_end, screen, stdout])?;

    for target in [libc::STDOUT_FILENO, libc::STDERR_FILENO] {
        if unsafe { libc::dup2(write_end, target) } < 0 {
            let error = io::Error::last_os_error();
            unsafe {
                libc::dup2(stdout, libc::STDOUT_FILENO);
                libc::dup2(stderr, libc::STDERR_FILENO);
            }
            close(&[read_end, write_end, screen, stdout, stderr]);
            return Err(error);
        }
    }
    unsafe { libc::close(write_end) };

    let reader = match spawn_reader(unsafe { File::from_raw_fd(read_end) }, dashboard) {
        Ok(reader) => reader,
        Err(error) => {
            unsafe {
                libc::dup2(stdout, libc::STDOUT_FILENO);
                libc::dup2(stderr, libc::STDERR_FILENO);
                libc::close(stdout);
                libc::close(stderr);
                libc::close(screen);
            }
            return Err(error);
        }
    };

    Ok((
        Redirect {
            stdout,
            stderr,
            reader: Some(reader),
        },
        unsafe { File::from_raw_fd(screen) },
    ))
}

fn pipe() -> io::Result<(RawFd, RawFd)> {
    let mut ends = [0; 2];
    if unsafe { libc::pipe(ends.as_mut_ptr()) } != 0 {
        return Err(io::Error::last_os_error());
    }
    if let Err(error) = set_close_on_exec(ends[0]).and_then(|()| set_close_on_exec(ends[1])) {
        close(&ends);
        return Err(error);
    }
    Ok((ends[0], ends[1]))
}

fn duplicate(fd: RawFd, opened: &[RawFd]) -> io::Result<RawFd> {
    let copy = unsafe { libc::dup(fd) };
    if copy < 0 {
        let error = io::Error::last_os_error();
        close(opened);
        return Err(error);
    }
    if let Err(error) = set_close_on_exec(copy) {
        close(&[copy]);
        close(opened);
        return Err(error);
    }
    Ok(copy)
}

fn set_close_on_exec(fd: RawFd) -> io::Result<()> {
    let flags = unsafe { libc::fcntl(fd, libc::F_GETFD) };
    if flags < 0 || unsafe { libc::fcntl(fd, libc::F_SETFD, flags | libc::FD_CLOEXEC) } < 0 {
        return Err(io::Error::last_os_error());
    }
    Ok(())
}

fn close(fds: &[RawFd]) {
    for fd in fds {
        unsafe { libc::close(*fd) };
    }
}
