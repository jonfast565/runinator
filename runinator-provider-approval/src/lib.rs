mod errors;

use std::sync::Arc;

use runinator_models::json;
use runinator_models::value::{Map, Value};
use runinator_models::{
    errors::SendableError,
    providers::{
        ActionMetadata, ParameterMetadata, ProviderMetadata, ProviderRuntimeMetadata,
        ResultMetadata, RuninatorType,
    },
    runs::{ProviderExecutionRequest, TaskExecutionResult},
};
use runinator_plugin::provider::{Provider, ProviderEventSink};
use serde::{Deserialize, Serialize};

fn parse_params(request: &ProviderExecutionRequest) -> Result<ApprovalParams, SendableError> {
    runinator_provider_support::parse_params(request, &errors::INVALID_PARAMS)
}

#[cfg(test)]
mod tests;

mod approval_params;
use approval_params::ApprovalParams;

mod approval_result;
use approval_result::ApprovalResult;

mod approval_provider;
pub use approval_provider::ApprovalProvider;
