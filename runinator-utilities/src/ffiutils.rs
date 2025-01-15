use std::ffi::{c_char, CStr};

pub fn str_to_c_string(some_str: &str) -> *const c_char {
    some_str.as_ptr() as *const c_char
}

pub fn cstr_to_rust_string(call: *const c_char) -> String {
    let c_str: &CStr = unsafe { CStr::from_ptr(call) };
    let str_slice: &str = c_str.to_str().unwrap();
    let str_buf: String = str_slice.to_owned();
    str_buf
}