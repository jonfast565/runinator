mod ffi;
mod linux;
mod model;
mod runner;
mod windows;

pub use ffi::{call_service, metadata, name, runinator_abi_version, runinator_marker};
