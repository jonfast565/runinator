use libloading::{Library, Symbol};
use runinator_models::errors::{RuntimeError, SendableError};
use runinator_utilities::ffiutils;
use std::{
    ffi::{CString, c_char, c_int},
    path::PathBuf,
};

use crate::provider::Provider;

const PLUGIN_MARKER_FN_NAME: &str = "runinator_marker\0";
const PLUGIN_SERVICE_CALL_FN_NAME: &str = "call_service\0";
const PLUGIN_NAME_FN_NAME: &str = "name\0";

type PluginServiceCallFn =
    unsafe extern "C" fn(call: *const c_char, args: *const c_char, timeout_secs: i64) -> c_int;
type PluginMarkerFn = unsafe extern "C" fn() -> c_int;
type PluginNameFn = unsafe extern "C" fn() -> *const c_char;

#[derive(Clone)]
pub struct Plugin {
    pub file_name: PathBuf,
    pub name: String,
}

impl Provider for Plugin {
    fn name(&self) -> String {
        self.name.clone()
    }

    fn call_service(
        &self,
        call: String,
        args: String,
        timeout_secs: i64,
    ) -> Result<i32, SendableError> {
        self.plugin_service_call(call, args, timeout_secs)?;
        Ok(0)
    }
}

impl Plugin {
    pub fn new(path: &PathBuf) -> Result<Self, SendableError> {
        let lib = unsafe { Library::new(path)? };

        let marker_symbol: Symbol<PluginMarkerFn> =
            unsafe { lib.get(PLUGIN_MARKER_FN_NAME.as_bytes())? };

        let name_symbol: Symbol<PluginNameFn> = unsafe { lib.get(PLUGIN_NAME_FN_NAME.as_bytes())? };

        let marker_result = unsafe { (marker_symbol)() };
        if marker_result != 1 {
            return Err(Box::new(RuntimeError::new(
                "1".to_string(),
                "Marker function did not return expected value".to_string(),
            )));
        }

        let name = unsafe { name_symbol() };
        let name_str_buf = ffiutils::cstr_to_rust_string(name);

        Ok(Plugin {
            name: name_str_buf,
            file_name: path.clone(),
        })
    }

    pub fn plugin_service_call(
        &self,
        name: String,
        args: String,
        timeout_secs: i64,
    ) -> Result<(), SendableError> {
        unsafe {
            let lib = { Library::new(self.file_name.clone())? };
            let service_call_symbol: Symbol<PluginServiceCallFn> =
                lib.get(PLUGIN_SERVICE_CALL_FN_NAME.as_bytes())?;
            let name_cstr = CString::new(name).unwrap();
            let args_cstr = CString::new(args).unwrap();
            let plugin_interface =
                (service_call_symbol)(name_cstr.as_ptr(), args_cstr.as_ptr(), timeout_secs);
            if plugin_interface != 0 {
                return Err(Box::new(RuntimeError::new(
                    "2".to_string(),
                    "Plugin execution failed".to_string(),
                )));
            }
            Ok(())
        }
    }
}
