use ctor::ctor;
use log::error;
use runinator_models::providers::{ActionMetadata, ProviderMetadata};
use runinator_utilities::{ffiutils, logger};
use std::ffi::{c_char, c_int};

use crate::runner::execute_request;

const NAME: &str = "Console\0";
const METADATA: &str = "{\"name\":\"Console\",\"actions\":[{\"function_name\":\"run\",\"description\":\"Run a shell command\",\"parameters\":[{\"name\":\"command\",\"ty\":{\"type\":\"string\"},\"required\":true}],\"results\":[{\"name\":\"success\",\"ty\":{\"type\":\"boolean\"}},{\"name\":\"exit_code\",\"ty\":{\"type\":\"integer\"}},{\"name\":\"command\",\"ty\":{\"type\":\"string\"}}]}],\"metadata\":{}}\0";

#[ctor(unsafe)]
fn constructor() {
    if let Err(err) = logger::setup_logger() {
        eprintln!("logger not set up: {err}");
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn runinator_marker() -> c_int {
    1
}

#[unsafe(no_mangle)]
pub extern "C" fn name() -> *const c_char {
    ffiutils::str_to_c_string(NAME)
}

#[unsafe(no_mangle)]
pub extern "C" fn metadata() -> *const c_char {
    let _: ProviderMetadata =
        serde_json::from_str(METADATA.trim_end_matches('\0')).unwrap_or_else(|_| {
            ProviderMetadata {
                name: "Console".into(),
                actions: vec![ActionMetadata::new("run", "Run a shell command")],
                metadata: Default::default(),
            }
        });
    ffiutils::str_to_c_string(METADATA)
}

#[unsafe(no_mangle)]
pub extern "C" fn runinator_abi_version() -> c_int {
    1
}

#[unsafe(no_mangle)]
pub extern "C" fn call_service(
    request_json_path: *const c_char,
    response_json_path: *const c_char,
) -> c_int {
    let request_path = match ffiutils::try_cstr_to_rust_string(request_json_path) {
        Ok(path) => path,
        Err(err) => {
            error!("Invalid request path from host: {}", err);
            return -1;
        }
    };
    let response_path = match ffiutils::try_cstr_to_rust_string(response_json_path) {
        Ok(path) => path,
        Err(err) => {
            error!("Invalid response path from host: {}", err);
            return -1;
        }
    };

    execute_request(&request_path, &response_path).unwrap_or_else(|e| {
        error!("Error executing command: {}", e);
        -1
    })
}
