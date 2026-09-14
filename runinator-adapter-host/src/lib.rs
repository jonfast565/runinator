//! hostable loopback adapter service used by the standalone and sidecar binaries.

#[path = "main.rs"]
mod implementation;

pub use implementation::{run_child_command, serve};
