use std::ffi::{c_char, c_int};
use log::info;
use runinator_utilities::{ffiutils, logger};

const NAME: &str = "SQL\0";

#[no_mangle]
extern "C" fn runinator_marker() -> c_int {
    1
}

#[no_mangle]
extern "C" fn name() -> *const c_char {
    ffiutils::str_to_c_string(NAME)
}

#[no_mangle]
extern "C" fn call_service(call: *const c_char, args: *const c_char) -> c_int {
    logger::setup_logger().unwrap();
    
    let call_str: String = ffiutils::cstr_to_rust_string(call);
    let args_str: String = ffiutils::cstr_to_rust_string(args);

    info!("Running action '{}' w/ args `{}`", call_str, args_str);

    0
}