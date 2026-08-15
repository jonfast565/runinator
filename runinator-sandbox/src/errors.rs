//! the `SANDBOX` error dictionary.

use runinator_models::errors::{EngineErrors, ErrorDescriptor};
use std::fmt;

/// what went wrong running a container.
///
/// timeout and cancellation are separate variants rather than one "aborted": a caller retries a
/// timeout and does not retry a cancel, and collapsing them would lose that.
#[derive(Debug)]
pub enum SandboxError {
    /// the container runtime could not be started at all (not installed, not running, no socket).
    RuntimeUnavailable(String),
    /// the spec described something that could not be run.
    InvalidSpec(String),
    /// the container ran past its deadline and was removed.
    TimedOut(u64),
    /// the caller cancelled the run and the container was removed.
    Cancelled,
    /// the container ran to completion with a non-zero exit code.
    Failed { exit_code: i32, stderr: String },
    /// the host side failed: a mount could not be prepared, a pipe could not be read.
    Io(String),
}

impl fmt::Display for SandboxError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::RuntimeUnavailable(detail) => {
                write!(
                    formatter,
                    "SANDBOX001 - container runtime unavailable: {detail}"
                )
            }
            Self::InvalidSpec(detail) => {
                write!(formatter, "SANDBOX002 - invalid container spec: {detail}")
            }
            Self::TimedOut(seconds) => write!(
                formatter,
                "SANDBOX003 - container timed out: exceeded {seconds} seconds"
            ),
            Self::Cancelled => write!(formatter, "SANDBOX004 - container canceled: run canceled"),
            Self::Failed { exit_code, stderr } => write!(
                formatter,
                "SANDBOX005 - container failed: exited with code {exit_code}: {stderr}"
            ),
            Self::Io(detail) => write!(formatter, "SANDBOX006 - container io error: {detail}"),
        }
    }
}

impl std::error::Error for SandboxError {}

pub const RUNTIME_UNAVAILABLE: ErrorDescriptor = ErrorDescriptor::new(
    "SANDBOX001",
    "sandbox.runtime_unavailable",
    "Container runtime unavailable",
);
pub const INVALID_SPEC: ErrorDescriptor = ErrorDescriptor::new(
    "SANDBOX002",
    "sandbox.invalid_spec",
    "Invalid container spec",
);
pub const TIMED_OUT: ErrorDescriptor =
    ErrorDescriptor::new("SANDBOX003", "sandbox.timed_out", "Container timed out");
pub const CANCELED: ErrorDescriptor =
    ErrorDescriptor::new("SANDBOX004", "sandbox.canceled", "Container canceled");
pub const FAILED: ErrorDescriptor =
    ErrorDescriptor::new("SANDBOX005", "sandbox.failed", "Container failed");
pub const IO: ErrorDescriptor =
    ErrorDescriptor::new("SANDBOX006", "sandbox.io", "Container io error");

pub const DICTIONARY: &[ErrorDescriptor] = &[
    RUNTIME_UNAVAILABLE,
    INVALID_SPEC,
    TIMED_OUT,
    CANCELED,
    FAILED,
    IO,
];

impl EngineErrors for SandboxError {
    fn error_dictionary() -> &'static [ErrorDescriptor] {
        DICTIONARY
    }
}
