use super::*;
use std::ffi::CString;

#[test]
fn converts_valid_ffi_strings_and_rejects_null() {
    let value = CString::new("plugin").unwrap();
    assert_eq!(
        unsafe { cstr_to_rust_string(value.as_ptr()) }.unwrap(),
        "plugin"
    );
    assert!(unsafe { cstr_to_rust_string(std::ptr::null()) }.is_err());
}

#[test]
fn reports_interior_nuls_in_file_paths() {
    let error = path_to_cstring(std::path::Path::new("bad\0path"), "request").unwrap_err();
    assert!(matches!(
        error,
        FileOperationError::InvalidPath {
            kind: "request",
            ..
        }
    ));
}
