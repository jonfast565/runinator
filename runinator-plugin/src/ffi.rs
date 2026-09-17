use std::{
    error::Error,
    ffi::{CStr, CString, NulError, c_char},
    fmt,
    path::{Path, PathBuf},
};

use libloading::{Library, Symbol};

pub type MarkerFn = unsafe extern "C" fn() -> u32;
pub type NameFn = unsafe extern "C" fn() -> *const c_char;
pub type FileOperationFn = unsafe extern "C" fn(*const c_char, *const c_char) -> std::ffi::c_int;

/// Resolve and invoke a typed function from a dynamic library.
///
/// # Safety
///
/// The symbol must have the function-pointer type supplied by `T` and remain valid for the
/// duration of the invocation.
pub unsafe fn call_symbol<T: Copy, R>(
    library: &Library,
    symbol: &[u8],
    invoke: impl FnOnce(T) -> R,
) -> Result<R, libloading::Error> {
    let symbol: Symbol<'_, T> = unsafe { library.get(symbol) }?;
    Ok(invoke(*symbol))
}

/// Resolve and invoke a marker function from a dynamic library.
///
/// # Safety
///
/// The symbol must have the [`MarkerFn`] ABI and remain valid for the duration of the invocation.
pub unsafe fn find_marker<T: Copy, R>(
    library: &Library,
    symbol: &[u8],
    invoke: impl FnOnce(T) -> R,
) -> Result<R, libloading::Error> {
    // SAFETY: forwarded to the caller's typed marker symbol contract.
    unsafe { call_symbol(library, symbol, invoke) }
}

/// Convert a non-null, NUL-terminated C string returned across an FFI boundary into an owned Rust
/// string.
///
/// # Safety
///
/// `value` must point to readable memory containing a NUL-terminated string for the duration of
/// this call.
pub unsafe fn cstr_to_rust_string(value: *const c_char) -> Result<String, std::io::Error> {
    if value.is_null() {
        return Err(std::io::Error::other("FFI string pointer was null"));
    }
    // SAFETY: upheld by this function's caller contract after the null check above.
    let value = unsafe { CStr::from_ptr(value) };
    value
        .to_str()
        .map(str::to_owned)
        .map_err(std::io::Error::other)
}

/// Convert a filesystem path to the C string expected by a file-based plugin ABI.
pub fn path_to_cstring(path: &Path, kind: &'static str) -> Result<CString, FileOperationError> {
    CString::new(path.to_string_lossy().as_bytes()).map_err(|source| {
        FileOperationError::InvalidPath {
            kind,
            path: path.to_owned(),
            source,
        }
    })
}

/// Invoke a two-path file operation after constructing bounded C strings for its arguments.
///
/// # Safety
///
/// `operation` must be a valid function pointer with the file-operation ABI.
pub unsafe fn invoke_file_operation(
    operation: FileOperationFn,
    request: Option<&Path>,
    response: &Path,
) -> Result<(), FileOperationError> {
    let request = match request {
        Some(path) => path_to_cstring(path, "request")?,
        None => CString::default(),
    };
    let response = path_to_cstring(response, "response")?;
    // SAFETY: the caller guarantees the loaded function has the file-operation ABI, and the
    // strings remain alive for the duration of this call.
    if unsafe { operation(request.as_ptr(), response.as_ptr()) } != 0 {
        return Err(FileOperationError::Failed);
    }
    Ok(())
}

#[derive(Debug)]
pub enum FileOperationError {
    InvalidPath {
        kind: &'static str,
        path: PathBuf,
        source: NulError,
    },
    Failed,
}

impl fmt::Display for FileOperationError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidPath { kind, path, source } => write!(
                formatter,
                "{kind} path contains an interior nul byte: {} ({source})",
                path.display()
            ),
            Self::Failed => formatter.write_str("file operation failed"),
        }
    }
}

impl Error for FileOperationError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::InvalidPath { source, .. } => Some(source),
            Self::Failed => None,
        }
    }
}

#[cfg(test)]
#[path = "ffi_tests.rs"]
mod tests;
