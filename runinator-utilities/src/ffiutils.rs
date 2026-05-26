use std::ffi::{CStr, c_char};

use runinator_models::errors::{RuntimeError, SendableError};

pub fn str_to_c_string(some_str: &str) -> *const c_char {
    some_str.as_ptr() as *const c_char
}

pub fn cstr_to_rust_string(call: *const c_char) -> String {
    try_cstr_to_rust_string(call).unwrap_or_default()
}

pub fn try_cstr_to_rust_string(call: *const c_char) -> Result<String, SendableError> {
    if call.is_null() {
        return Err(Box::new(RuntimeError::new(
            "ffi.null_string".into(),
            "FFI string pointer was null".into(),
        )));
    }

    let c_str: &CStr = unsafe { CStr::from_ptr(call) };
    Ok(c_str.to_str()?.to_owned())
}
