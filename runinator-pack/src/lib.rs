//! pack source compilation: turn a `.wdl`/`.wdlm`/directory (plus an adjacent `.wdls`/`.json`
//! settings file) into a `WorkflowBundle`/`SecretBundle` ready for `/packs/import`. shared by the
//! control cli and the language server so the compile-to-bundle path lives in one place.

pub mod errors;
pub mod source;

pub use errors::{PackError, Result};
