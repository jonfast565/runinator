//! open the command-center UI, either as a native app (e.g. a Tauri `.app` bundle on macOS, or an
//! executable path on Windows/Linux) if one is configured, or as a URL in the default browser
//! otherwise. both cases go through the same `open` crate call — on macOS it shells to `open <path>`,
//! which launches an app bundle exactly the same way it opens a URL, so no platform branching is
//! needed here.

/// prefer launching `app_path` (a native command-center install) when set; fall back to opening
/// `url` in the default browser.
pub fn open_command_center(app_path: &str, url: &str) -> Result<(), String> {
    if !app_path.trim().is_empty() {
        return open::that(app_path).map_err(|err| err.to_string());
    }
    if url.trim().is_empty() {
        return Err("no command-center app or URL configured".to_string());
    }
    open::that(url).map_err(|err| err.to_string())
}
