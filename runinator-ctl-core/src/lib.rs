//! Portable `runinatorctl` command language.
//!
//! This crate owns the clap tree and the console parser/catalog derived from it. Native
//! `runinatorctl` and the browser WASM adapter both depend on this crate, so command validation,
//! help, and completion do not drift between front ends.

pub mod cli;
pub mod console;
