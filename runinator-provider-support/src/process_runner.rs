//! non-interactive process execution with concurrent output draining.
use crate::process::{ProcessOutput, ProcessOutputPump};
use runinator_plugin::{cancel::CancellationToken, provider::ProviderEventSink};
use std::{
    io::{self, Write},
    process::{Command, ExitStatus, Stdio},
    sync::Arc,
    thread,
    time::{Duration, Instant},
};

/// providers map failures into their own numbered error dictionaries.
#[derive(Debug)]
pub enum ProcessFailure {
    Spawn(io::Error),
    Io(io::Error),
    Canceled,
    TimedOut,
}

#[cfg(test)]
#[path = "process_runner_tests.rs"]
mod tests;

mod process_request;
pub use process_request::ProcessRequest;

mod process_result;
pub use process_result::ProcessResult;

mod process_runner;
pub use process_runner::ProcessRunner;

mod native_process_runner;
pub use native_process_runner::NativeProcessRunner;
