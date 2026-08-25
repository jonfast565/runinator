//! Windows implementation of the dashboard stream capture.
//!
//! Rust resolves stdout/stderr through `GetStdHandle` for each write, while crossterm opens the
//! active console by name. Swapping the standard handles therefore catches direct writes without
//! changing the handle used by the dashboard renderer.

use std::fs::File;
use std::io::{self, Write};
use std::os::windows::io::{AsRawHandle, FromRawHandle};
use std::sync::Arc;
use std::thread::JoinHandle;

use windows_sys::Win32::Foundation::{HANDLE, INVALID_HANDLE_VALUE};
use windows_sys::Win32::Globalization::CP_UTF8;
use windows_sys::Win32::Storage::FileSystem::{
    CreateFileW, FILE_SHARE_READ, FILE_SHARE_WRITE, OPEN_EXISTING,
};
use windows_sys::Win32::System::Console::{
    GetConsoleOutputCP, GetStdHandle, STD_ERROR_HANDLE, STD_HANDLE, STD_OUTPUT_HANDLE,
    SetConsoleOutputCP, SetStdHandle,
};
use windows_sys::Win32::System::Pipes::CreatePipe;

use super::{Dashboard, Screen, spawn_reader};

const FAILED: i32 = 0;

pub(super) struct Redirect {
    stdout: HANDLE,
    stderr: HANDLE,
    write: Option<File>,
    code_page: Option<u32>,
    reader: Option<JoinHandle<()>>,
}

unsafe impl Send for Redirect {}

impl Redirect {
    pub(super) fn restore(mut self) {
        let _ = io::stdout().flush();
        let _ = io::stderr().flush();
        unsafe {
            SetStdHandle(STD_OUTPUT_HANDLE, self.stdout);
            SetStdHandle(STD_ERROR_HANDLE, self.stderr);
        }
        if let Some(code_page) = self.code_page.take() {
            unsafe { SetConsoleOutputCP(code_page) };
        }
        drop(self.write.take());
        if let Some(reader) = self.reader.take() {
            let _ = reader.join();
        }
    }
}

pub(super) fn install(dashboard: Arc<Dashboard>) -> io::Result<(Redirect, Screen)> {
    io::stdout().flush()?;
    io::stderr().flush()?;
    let stdout = std_handle(STD_OUTPUT_HANDLE)?;
    let stderr = std_handle(STD_ERROR_HANDLE)?;
    let screen = console_screen()?;
    let code_page = utf8_output();
    let (read_end, write_end) = pipe()?;
    let sink = write_end.as_raw_handle() as HANDLE;

    for stream in [STD_OUTPUT_HANDLE, STD_ERROR_HANDLE] {
        if unsafe { SetStdHandle(stream, sink) } == FAILED {
            let error = io::Error::last_os_error();
            undo(stdout, stderr, code_page);
            return Err(error);
        }
    }
    let reader = match spawn_reader(read_end, dashboard) {
        Ok(reader) => reader,
        Err(error) => {
            undo(stdout, stderr, code_page);
            return Err(error);
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
    ))
}

fn undo(stdout: HANDLE, stderr: HANDLE, code_page: Option<u32>) {
    unsafe {
        SetStdHandle(STD_OUTPUT_HANDLE, stdout);
        SetStdHandle(STD_ERROR_HANDLE, stderr);
    }
    if let Some(code_page) = code_page {
        unsafe { SetConsoleOutputCP(code_page) };
    }
}

fn std_handle(stream: STD_HANDLE) -> io::Result<HANDLE> {
    let handle = unsafe { GetStdHandle(stream) };
    if handle.is_null() || handle == INVALID_HANDLE_VALUE {
        return Err(io::Error::other(
            "this process has no standard output to redirect",
        ));
    }
    Ok(handle)
}

fn console_screen() -> io::Result<File> {
    let name: Vec<u16> = "CONOUT$\0".encode_utf16().collect();
    let handle = unsafe {
        CreateFileW(
            name.as_ptr(),
            windows_sys::Win32::Foundation::GENERIC_READ
                | windows_sys::Win32::Foundation::GENERIC_WRITE,
            FILE_SHARE_READ | FILE_SHARE_WRITE,
            std::ptr::null(),
            OPEN_EXISTING,
            0,
            std::ptr::null_mut(),
        )
    };
    if handle.is_null() || handle == INVALID_HANDLE_VALUE {
        return Err(io::Error::last_os_error());
    }
    Ok(unsafe { File::from_raw_handle(handle as _) })
}

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

fn pipe() -> io::Result<(File, File)> {
    let mut read = std::ptr::null_mut();
    let mut write = std::ptr::null_mut();
    if unsafe { CreatePipe(&mut read, &mut write, std::ptr::null(), 0) } == FAILED {
        return Err(io::Error::last_os_error());
    }
    Ok(unsafe {
        (
            File::from_raw_handle(read as _),
            File::from_raw_handle(write as _),
        )
    })
}
