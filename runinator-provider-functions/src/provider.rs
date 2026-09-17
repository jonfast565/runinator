//! the `functions` provider: one action, `invoke`.

use std::sync::Arc;

use runinator_models::{
    errors::SendableError,
    functions::{FUNCTIONS_INVOKE, FUNCTIONS_PROVIDER},
    providers::ProviderMetadata,
    runs::{ProviderExecutionEvent, ProviderExecutionRequest, TaskExecutionResult},
};
use runinator_plugin::{
    cancel::CancellationToken,
    provider::{Provider, ProviderEventSink},
};
use runinator_sandbox::{LineSink, Stream};

use crate::errors::INVALID_REQUEST;
use crate::request::InvocationRequest;
use crate::runtime::{DockerInvocationRuntime, InvocationRuntime};

/// executes packaged functions staged by the worker.

// bridges the sandbox's line sink onto the worker's chunk events.

#[cfg(test)]
#[path = "provider_tests.rs"]
mod tests;

mod functions_provider;
pub use functions_provider::FunctionsProvider;

mod event_sink;
use event_sink::EventSink;
