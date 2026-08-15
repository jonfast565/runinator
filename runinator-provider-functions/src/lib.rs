//! executing packaged functions: the worker-side half of published code.
//!
//! the provider advertises exactly one action, `invoke`. per-export action names would be rejected,
//! because the worker validates an action's function against `Provider::metadata()` before executing
//! and no static metadata can enumerate every export ever published — the export is named by the
//! `FunctionBinding` on the action instead.
//!
//! it is deliberately free of control-plane calls. by the time a request arrives, the worker has
//! already downloaded the artifact, verified its digest, and unpacked it, and passes the local path
//! in; the provider mounts that directory and runs it. that split is what lets the same provider run
//! on a host worker, a desktop agent, or (later) a kubernetes job without any of them knowing how
//! the other fetched the code.

mod errors;
mod languages;
mod provider;
mod request;
mod runtime;

pub use errors::DICTIONARY;
pub use languages::{RuntimeAdapter, adapter_for, default_image};
pub use provider::FunctionsProvider;
pub use request::{
    CONTEXT_KEY, HANDLER_KEY, INPUT_KEY, InvocationRequest, LIMITS_KEY, PACKAGE_PATH_KEY,
    RUNTIME_KEY,
};
pub use runtime::{DockerInvocationRuntime, InvocationOutcome, InvocationRuntime};
