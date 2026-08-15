//! running untrusted code in a container, with a bounded envelope around it.
//!
//! the reusable thing here is *container execution*, not any one caller's idea of what it is
//! running: `std.code` executes an author's inline snippet, and a packaged function executes a
//! published archive, but both need the same four things — a bounded runtime, bounded output,
//! cancellation that actually removes the container, and resource caps that a hostile payload
//! cannot talk its way out of.
//!
//! the crate knows nothing about workflows, providers, or the control plane. it takes a
//! [`ContainerSpec`], runs it, and reports what happened.
//!
//! ## Bounded output is not optional
//!
//! the obvious implementation — spawn the process with piped stdout/stderr, poll `try_wait`, then
//! call `wait_with_output` — **deadlocks** on any payload that writes more than a pipe buffer: the
//! child blocks writing, so it never exits, so `try_wait` never reports it, and the run dies on its
//! timeout instead of succeeding. [`docker::DockerRunner`] drains both streams on their own threads
//! for exactly this reason, and truncates at [`SandboxLimits::max_output_bytes`] so a chatty
//! payload cannot exhaust the host's memory either.

pub mod docker;
pub mod errors;
mod runner;
mod spec;

pub use docker::DockerRunner;
pub use errors::SandboxError;
pub use runner::{CancelSignal, ContainerRunner, LineSink, Stream, never_cancelled};
pub use spec::{ContainerOutput, ContainerSpec, Mount, SandboxLimits};

pub type Result<T> = std::result::Result<T, SandboxError>;
