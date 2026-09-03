use std::ffi::{CStr, c_char};

use runinator_models::errors::SendableError;

pub fn str_to_c_string(some_str: &str) -> *const c_char {
    some_str.as_ptr() as *const c_char
}

/// Convert a non-null, valid NUL-terminated C string to an owned Rust string.
///
/// # Safety
///
/// `call` must point to readable memory containing a NUL-terminated C string for the duration of
/// this call.
pub unsafe fn cstr_to_rust_string(call: *const c_char) -> String {
    // SAFETY: upheld by this function's caller contract.
    unsafe { try_cstr_to_rust_string(call) }.unwrap_or_default()
}

/// Convert a non-null, valid NUL-terminated C string to an owned Rust string.
///
/// # Safety
///
/// `call` must point to readable memory containing a NUL-terminated C string for the duration of
/// this call.
pub unsafe fn try_cstr_to_rust_string(call: *const c_char) -> Result<String, SendableError> {
    if call.is_null() {
        return Err(crate::errors::FFI_NULL_STRING.bare());
    }

    // SAFETY: upheld by this function's caller contract after the null check above.
    let c_str: &CStr = unsafe { CStr::from_ptr(call) };
    Ok(c_str.to_str()?.to_owned())
}
