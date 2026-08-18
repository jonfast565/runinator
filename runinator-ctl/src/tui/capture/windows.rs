//! moving the standard streams with `SetStdHandle`.
//!
//! `println!` here does not travel through descriptor 1, but it does call
//! `GetStdHandle(STD_OUTPUT_HANDLE)` on *every* write rather than caching what it gets back
//! (`sys::stdio::windows`), so `SetStdHandle` moves the whole process's output the way `dup2` does
//! on unix — a dependency's `println!` included.
//!
//! what makes this safe to do under a running interface is that crossterm does not use the std
//! handles at all: `crossterm_winapi::Handle::current_out_handle` opens `CONOUT$` by name with
//! `CreateFileW`, and `current_in_handle` opens `CONIN$`. the terminal size, raw mode, the
//! alternate screen, cursor visibility, and the event source all resolve through those, so none of
//! them can see the redirection. this module opens `CONOUT$` the same way for the same reason: a
//! duplicate of the original stdout would be the *old* screen buffer the moment anything switched
//! buffers, where `CONOUT$` always names the active one.
//!
//! the console's output code page is the one other thing that has to be arranged. the interface
//! draws through a `File`, which reaches the console as raw bytes — unlike rust's own `Stdout`,
//! which converts to utf-16 first — so the box drawing and the `·` in the status line need the
//! console reading those bytes as utf-8, which most windows consoles do not do by default.

use std::fs::File;
use std::io::{self, Write};
use std::os::windows::io::{AsRawHandle, FromRawHandle};
use std::thread::JoinHandle;

use windows_sys::Win32::Foundation::{
    DUPLICATE_SAME_ACCESS, DuplicateHandle, GENERIC_READ, GENERIC_WRITE, HANDLE,
    INVALID_HANDLE_VALUE,
};
use windows_sys::Win32::Globalization::CP_UTF8;
use windows_sys::Win32::Storage::FileSystem::{
    CreateFileW, FILE_SHARE_READ, FILE_SHARE_WRITE, OPEN_EXISTING,
};
use windows_sys::Win32::System::Console::{
    GetConsoleOutputCP, GetStdHandle, STD_ERROR_HANDLE, STD_HANDLE, STD_OUTPUT_HANDLE,
    SetConsoleOutputCP, SetStdHandle,
};
use windows_sys::Win32::System::Pipes::CreatePipe;
use windows_sys::Win32::System::Threading::GetCurrentProcess;

use super::{Screen, Shared, spawn_reader};
use crate::commands::{Result, err};

/// the win32 `BOOL` a failed call returns.
const FAILED: i32 = 0;

pub(super) struct Redirect {
    /// what `GetStdHandle` returned before the swap. borrowed rather than owned — `SetStdHandle`
    /// does not close what it replaces, so there is nothing here to close on the way out.
    stdout: HANDLE,
    stderr: HANDLE,
    /// this process's copy of the pipe's write end. the standard streams point at it but do not own
    /// it, so this is the last reference and dropping it is what ends the reader.
    write: Option<File>,
    /// the console's output code page before it was put into utf-8, when it had to be changed.
    code_page: Option<u32>,
    reader: Option<JoinHandle<()>>,
}

// `HANDLE` is a raw pointer, which is not `Send` by default. these are process-wide kernel objects
// rather than thread-owned state, and the console moves the whole `Capture` between turns.
unsafe impl Send for Redirect {}

impl Redirect {
    pub(super) fn restore(mut self) {
        // whatever is still buffered belongs in the transcript, not in the next thing written to the
        // restored terminal.
        let _ = io::stdout().flush();
        let _ = io::stderr().flush();
        unsafe {
            SetStdHandle(STD_OUTPUT_HANDLE, self.stdout);
            SetStdHandle(STD_ERROR_HANDLE, self.stderr);
        }
        if let Some(code_page) = self.code_page.take() {
            unsafe { SetConsoleOutputCP(code_page) };
        }
        // the standard streams no longer reference the write end, so this is the last handle to it.
        // closing it is what turns the reader's next read into a broken pipe, which is its
        // end-of-file — the same moment the unix half reaches by closing descriptors 1 and 2.
        drop(self.write.take());
        if let Some(reader) = self.reader.take() {
            let _ = reader.join();
        }
    }
}

pub(super) fn install(limit: usize) -> Result<(Redirect, Screen, Shared)> {
    // anything already buffered was written for the terminal, so it goes there before the pipe takes
    // the stream.
    io::stdout().flush()?;

    let stdout = std_handle(STD_OUTPUT_HANDLE)?;
    let stderr = std_handle(STD_ERROR_HANDLE)?;
    let screen = console_screen().or_else(|_| duplicate(stdout))?;
    let code_page = utf8_output();

    let (read_end, write_end) = pipe()?;
    let sink = write_end.as_raw_handle() as HANDLE;
    for stream in [STD_OUTPUT_HANDLE, STD_ERROR_HANDLE] {
        if unsafe { SetStdHandle(stream, sink) } == FAILED {
            let failure = io::Error::last_os_error();
            undo(stdout, stderr, code_page);
            return Err(err(format!("cannot redirect output: {failure}")));
        }
    }

    // a reader that never started would leave the streams pointing into a pipe nothing drains, which
    // blocks the first command that fills it.
    let (reader, transcript) = match spawn_reader(read_end, limit) {
        Ok(started) => started,
        Err(failure) => {
            undo(stdout, stderr, code_page);
            return Err(failure);
        }
    };

    Ok((
        Redirect {
            stdout,
            stderr,
            write: Some(write_end),
            code_page,
            reader: Some(reader),
        },
        screen,
        transcript,
    ))
}

// put back whatever a failed install had already changed, so the process is left as it was found.
fn undo(stdout: HANDLE, stderr: HANDLE, code_page: Option<u32>) {
    unsafe {
        SetStdHandle(STD_OUTPUT_HANDLE, stdout);
        SetStdHandle(STD_ERROR_HANDLE, stderr);
    }
    if let Some(code_page) = code_page {
        unsafe { SetConsoleOutputCP(code_page) };
    }
}

/// the handle one of the standard streams currently points at.
fn std_handle(stream: STD_HANDLE) -> Result<HANDLE> {
    let handle = unsafe { GetStdHandle(stream) };
    if handle.is_null() || handle == INVALID_HANDLE_VALUE {
        return Err(err("this process has no standard output to redirect"));
    }
    Ok(handle)
}

/// the active console screen buffer, by name.
///
/// `CONOUT$` rather than a duplicate of stdout on purpose: it is the handle crossterm resolves for
/// its own calls, and it always names whichever buffer is active rather than whichever one was
/// active when the console started.
///
/// a process with no console at all has no `CONOUT$` to open, and `install` falls back to a
/// duplicate of stdout — the unix arrangement exactly. deciding whether what is there is a usable
/// terminal is `console.rs`'s job, and it makes that call with `is_terminal` before it ever gets
/// here; a second opinion in this module could only disagree with it.
fn console_screen() -> Result<File> {
    let name: Vec<u16> = "CONOUT$\0".encode_utf16().collect();
    let handle = unsafe {
        CreateFileW(
            name.as_ptr(),
            GENERIC_READ | GENERIC_WRITE,
            FILE_SHARE_READ | FILE_SHARE_WRITE,
            std::ptr::null(),
            OPEN_EXISTING,
            0,
            std::ptr::null_mut(),
        )
    };
    if handle.is_null() || handle == INVALID_HANDLE_VALUE {
        return Err(err(format!(
            "cannot open the console to draw on: {}",
            io::Error::last_os_error()
        )));
    }
    Ok(unsafe { File::from_raw_handle(handle as _) })
}

/// put the console into utf-8 for the duration, returning what it was if it had to change.
///
/// a console that refuses is not a reason to give up and fall back to the plain prompt: the
/// interface is still drawable, and what suffers is the handful of non-ascii characters in it.
fn utf8_output() -> Option<u32> {
    let current = unsafe { GetConsoleOutputCP() };
    if current == CP_UTF8 {
        return None;
    }
    match unsafe { SetConsoleOutputCP(CP_UTF8) } {
        FAILED => None,
        _ => Some(current),
    }
}

fn pipe() -> Result<(File, File)> {
    let mut read: HANDLE = std::ptr::null_mut();
    let mut write: HANDLE = std::ptr::null_mut();
    // no security attributes, so neither end is inheritable. nothing in ctl spawns a child process;
    // one that did would keep its output on the real terminal rather than in the pane, where the
    // unix half would have captured it.
    if unsafe { CreatePipe(&mut read, &mut write, std::ptr::null(), 0) } == FAILED {
        return Err(err(format!(
            "cannot open an output pipe: {}",
            io::Error::last_os_error()
        )));
    }
    Ok(unsafe {
        (
            File::from_raw_handle(read as _),
            File::from_raw_handle(write as _),
        )
    })
}

/// an owned duplicate of `handle`, for the fallback when there is no console to name.
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
