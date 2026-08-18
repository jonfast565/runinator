//! moving the standard streams with `SetStdHandle`.
//!
//! `println!` here does not travel through descriptor 1, which is why this is not the unix module
//! with the names changed. what it does travel through is `GetStdHandle(STD_OUTPUT_HANDLE)`, and
//! std calls that on *every* write rather than caching what it gets back (`sys::stdio::windows`) —
//! so `SetStdHandle` is the exact analogue of `dup2`: it moves every `println!` in the process, a
//! dependency's included, and a spawned child inherits the replacement the same way.
//!
//! two things are simpler than on unix. the sink is a `File` this module owns rather than a
//! descriptor number, so rewinding it is an ordinary `seek` instead of an `lseek` on a number; and
//! "leave nothing behind on any exit path" is a flag rather than the open-then-unlink dance. the
//! one thing that is worse: `FILE_FLAG_DELETE_ON_CLOSE` deletes when the last handle closes, so
//! unlike the unix file this one is visible on disk while it is being written.
//!
//! what is *not* redirected is the c runtime's own file descriptor 1, which `SetStdHandle` does not
//! move. no runinator command prints through c stdio; a dependency that did would escape the
//! capture and land in the middle of a frame.

use std::fs::{File, OpenOptions};
use std::io::{self, Read, Seek, SeekFrom, Write};
use std::os::windows::fs::OpenOptionsExt;
use std::os::windows::io::{AsRawHandle, FromRawHandle};

use windows_sys::Win32::Foundation::{
    DUPLICATE_SAME_ACCESS, DuplicateHandle, HANDLE, INVALID_HANDLE_VALUE,
};
use windows_sys::Win32::Storage::FileSystem::{
    FILE_FLAG_DELETE_ON_CLOSE, FILE_SHARE_DELETE, FILE_SHARE_READ, FILE_SHARE_WRITE,
};
use windows_sys::Win32::System::Console::{
    GetStdHandle, STD_ERROR_HANDLE, STD_HANDLE, STD_OUTPUT_HANDLE, SetStdHandle,
};
use windows_sys::Win32::System::Threading::GetCurrentProcess;

use super::{SCRATCH_LIMIT, scratch_path};
use crate::commands::{Result, err};

/// the win32 `BOOL` a failed call returns.
const FAILED: i32 = 0;

pub(super) struct Redirect {
    /// the scratch file, opened separately from the handle writing into it so it carries its own
    /// read position.
    reader: File,
    /// the handle the standard streams were pointed at. held for two reasons: dropping it would
    /// close the handle std is writing through, and rewinding means seeking it.
    sink: File,
    /// what `GetStdHandle` returned before the swap. borrowed rather than owned — `SetStdHandle`
    /// does not close what it replaces, so there is nothing here to close on the way out.
    stdout: HANDLE,
    stderr: HANDLE,
}

// `HANDLE` is a raw pointer, which is not `Send` by default. the handles here are process-wide
// kernel objects rather than thread-owned state, and `OutputCapture` is moved into the server
// before the first frame is read.
unsafe impl Send for Redirect {}

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
            SetStdHandle(STD_OUTPUT_HANDLE, self.stdout);
            SetStdHandle(STD_ERROR_HANDLE, self.stderr);
        }
    }

    // the sink and the reader each carry their own position in the file, so truncating means
    // seeking both back. doing it only past the limit keeps the ordinary path to a single read.
    fn rewind_when_large(&mut self) {
        let large = self
            .reader
            .stream_position()
            .is_ok_and(|position| position > SCRATCH_LIMIT);
        if !large || self.sink.set_len(0).is_err() {
            return;
        }
        let _ = self.reader.seek(SeekFrom::Start(0));
        // the sink is the handle std writes through, so its position is the write position.
        let _ = self.sink.seek(SeekFrom::Start(0));
    }
}

pub(super) fn install() -> Result<(Redirect, File)> {
    // anything already buffered was written for the terminal, so it goes there before the handle is
    // taken.
    io::stdout().flush()?;

    // every handle to the file has to permit the others, including the delete the sink is opened
    // with — a reader without `FILE_SHARE_DELETE` would make the second open fail.
    let share = FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE;
    let path = scratch_path();
    let sink = OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(true)
        .share_mode(share)
        // delete-on-close is the windows spelling of the unix unlink: the last handle to go takes
        // the file with it, however the process ends.
        .custom_flags(FILE_FLAG_DELETE_ON_CLOSE)
        .open(&path)
        .map_err(|failure| err(format!("cannot open a scratch file for output: {failure}")))?;
    let reader = OpenOptions::new()
        .read(true)
        .share_mode(share)
        .open(&path)
        .map_err(|failure| err(format!("cannot read back captured output: {failure}")))?;

    let stdout = std_handle(STD_OUTPUT_HANDLE)?;
    let stderr = std_handle(STD_ERROR_HANDLE)?;
    let screen = duplicate(stdout)?;

    let sink_handle = sink.as_raw_handle() as HANDLE;
    for stream in [STD_OUTPUT_HANDLE, STD_ERROR_HANDLE] {
        if unsafe { SetStdHandle(stream, sink_handle) } == FAILED {
            let failure = io::Error::last_os_error();
            // whichever half succeeded goes back, so a failed install leaves the process as it
            // found it rather than half redirected.
            unsafe {
                SetStdHandle(STD_OUTPUT_HANDLE, stdout);
                SetStdHandle(STD_ERROR_HANDLE, stderr);
            }
            return Err(err(format!("cannot redirect output: {failure}")));
        }
    }

    Ok((
        Redirect {
            reader,
            sink,
            stdout,
            stderr,
        },
        screen,
    ))
}

/// the handle one of the standard streams currently points at.
fn std_handle(stream: STD_HANDLE) -> Result<HANDLE> {
    let handle = unsafe { GetStdHandle(stream) };
    // a process started without standard streams — a detached gui launch — has nothing to move and
    // nothing to answer on.
    if handle.is_null() || handle == INVALID_HANDLE_VALUE {
        return Err(err(
            "this process has no standard output for the mcp protocol to answer on",
        ));
    }
    Ok(handle)
}

/// an owned duplicate of `handle`, for the protocol to answer on.
///
/// the original is left alone: a `File` closes what it holds, and closing the process's real stdout
/// is not something to do on the way to writing a frame to it.
fn duplicate(handle: HANDLE) -> Result<File> {
    let process = unsafe { GetCurrentProcess() };
    let mut copy: HANDLE = std::ptr::null_mut();
    let made = unsafe {
        DuplicateHandle(
            process,
            handle,
            process,
            &mut copy,
            0,
            // the duplicate is this process's protocol channel; a child has no business inheriting
            // it, and one that did could write frames of its own.
            FAILED,
            DUPLICATE_SAME_ACCESS,
        )
    };
    if made == FAILED {
        return Err(err(format!(
            "cannot duplicate the terminal: {}",
            io::Error::last_os_error()
        )));
    }
    Ok(unsafe { File::from_raw_handle(copy as _) })
}
