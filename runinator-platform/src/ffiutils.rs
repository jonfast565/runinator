use std::ffi::{CStr, c_char};

use runinator_models::errors::SendableError;

pub fn str_to_c_string(some_str: &str) -> *const c_char {
    some_str.as_ptr() as *const c_char
}

// callers own the contract that the pointer is a valid c string.
#[allow(clippy::not_unsafe_ptr_arg_deref)]
pub fn cstr_to_rust_string(call: *const c_char) -> String {
    try_cstr_to_rust_string(call).unwrap_or_default()
}

// callers own the contract that the pointer is a valid c string.
#[allow(clippy::not_unsafe_ptr_arg_deref)]
pub fn try_cstr_to_rust_string(call: *const c_char) -> Result<String, SendableError> {
    if call.is_null() {
        return Err(crate::errors::FFI_NULL_STRING.bare());
    }

    let c_str: &CStr = unsafe { CStr::from_ptr(call) };
    Ok(c_str.to_str()?.to_owned())
}
