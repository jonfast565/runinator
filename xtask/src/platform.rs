//! per-os naming conventions, mirroring the equivalents build.ps1 used to hardcode.

/// filename of the console plugin dynamic library for the current target os.
pub fn plugin_library_name() -> &'static str {
    if cfg!(target_os = "windows") {
        "runinator_plugin_console.dll"
    } else if cfg!(target_os = "macos") {
        "libruninator_plugin_console.dylib"
    } else {
        "libruninator_plugin_console.so"
    }
}

/// appends the platform executable suffix (`.exe` on windows, none elsewhere) to a binary name.
pub fn executable_name(name: &str) -> String {
    if cfg!(target_os = "windows") {
        format!("{name}.exe")
    } else {
        name.to_string()
    }
}
