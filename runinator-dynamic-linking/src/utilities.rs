use std::ffi::CStr;
use libloading::{Library, Symbol};

pub(crate) fn get_library_name(library_path: &str, marker_function: &str) -> Option<String> {
    let lib = unsafe { Library::new(library_path) };
    if let Ok(lib) = lib {
        unsafe {
            let name: Result<Symbol<unsafe extern "C" fn() -> *const u8>, _> =
                lib.get(marker_function.as_bytes());
            if let Ok(name) = name {
                let c_str = CStr::from_ptr(name() as *const i8);
                return c_str.to_str().ok().map(String::from);
            }
        }
    }
    None
}

pub(crate) fn get_library_extension() -> &'static str {
    if cfg!(target_os = "windows") {
        "dll"
    } else if cfg!(target_os = "macos") {
        "dylib"
    } else {
        "so"
    }
}
