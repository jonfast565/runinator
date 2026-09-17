//! Filesystem, process-lifecycle, shell, and FFI support shared by binaries and plugins.

pub mod app_data;
pub mod dirutils;
pub mod env;
pub mod errors;
pub mod ffiutils;
pub mod liveness;
pub mod shell;
pub mod startup;
pub mod time;
