//! pack source compilation: turn a `.rexrap`/`.rexrapm`/directory (plus an adjacent `.rexraps`/`.json`
//! settings file) into a `WorkflowBundle`/`SettingsBundle` ready for `/packs/import`. shared by the
//! control CLI and the language server so the compile-to-bundle path lives in one place.

pub mod errors;
pub mod functions;
pub mod source;

pub use errors::{PackError, Result};
