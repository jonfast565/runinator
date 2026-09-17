//! what to run, under what limits, and what came back.

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::time::Duration;

/// one mebibyte of captured output per stream, which is far more than a log ever needs and far less
/// than a payload can use to exhaust the host.
pub const DEFAULT_MAX_OUTPUT_BYTES: usize = 1024 * 1024;

mod mount;
pub use mount::Mount;

mod sandbox_limits;
pub use sandbox_limits::SandboxLimits;

mod container_spec;
pub use container_spec::ContainerSpec;

mod container_output;
pub use container_output::ContainerOutput;
