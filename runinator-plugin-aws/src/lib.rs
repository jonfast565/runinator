use std::ffi::{c_char, c_int, CStr};
use log::info;

const NAME: &str = "AWS\0";

#[no_mangle]
unsafe extern "C" fn runinator_marker() -> c_int {
    1
}

#[no_mangle]
unsafe extern "C" fn name() -> *const c_char {
    NAME.as_ptr() as *const c_char
}

#[no_mangle]
unsafe extern "C" fn call_service(call: *const c_char, args: *const c_char) -> c_int {
    let call_c_str: &CStr = unsafe { CStr::from_ptr(call) };
    let call_str_slice: &str = call_c_str.to_str().unwrap();
    let call_str_buf: String = call_str_slice.to_owned();

    let args_c_str: &CStr = unsafe { CStr::from_ptr(args) };
    let args_str_slice: &str = args_c_str.to_str().unwrap();
    let args_str_buf: String = args_str_slice.to_owned();

    info!("{} -> {}", call_str_buf, args_str_buf);
    0
}