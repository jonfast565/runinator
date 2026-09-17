use std::sync::{Arc, mpsc::Receiver};

use runinator_models::{
    errors::SendableError,
    providers::ProviderMetadata,
    runs::{
        ProviderExecutionEvent, ProviderExecutionRequest, ProviderTerminalControl,
        TaskExecutionResult,
    },
};

use crate::cancel::CancellationToken;

mod provider_event_sink;
pub use provider_event_sink::ProviderEventSink;

mod provider;
pub use provider::Provider;
