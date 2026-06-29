use std::sync::Arc;

use runinator_models::{
    errors::SendableError,
    providers::{
        ActionMetadata, ParameterMetadata, ProviderMetadata, ProviderRuntimeMetadata,
        ResultMetadata, RuninatorType,
    },
    runs::{ProviderExecutionRequest, TaskExecutionResult},
};
use runinator_plugin::cancel::CancellationToken;
use runinator_plugin::provider::{Provider, ProviderEventSink};

use crate::runner::execute;

/// reads, writes, and inspects files on the machine the worker runs on, confined to a configured
/// sandbox root. intended for an embedded desktop worker; it is deliberately kept out of the shared
/// server catalog so cloud workers never touch a user's disk. every result carries
/// `location: "local"` to distinguish on-disk files from cloud-stored artifacts.
#[derive(Clone)]
pub struct LocalProvider;

impl Provider for LocalProvider {
    fn name(&self) -> String {
        "local".to_string()
    }

    fn metadata(&self) -> ProviderMetadata {
        let path_param = || ParameterMetadata::required("path", RuninatorType::String);
        ProviderMetadata {
            name: self.name(),
            actions: vec![
                ActionMetadata::new("read_file", "Read a file from the local sandbox")
                    .with_parameters(vec![path_param()])
                    .with_results(vec![
                        ResultMetadata::new("path", RuninatorType::String),
                        ResultMetadata::new("size_bytes", RuninatorType::Integer),
                        ResultMetadata::new("mime_type", RuninatorType::String),
                        ResultMetadata::new("location", RuninatorType::String),
                        ResultMetadata::new("content", RuninatorType::String),
                    ]),
                ActionMetadata::new("write_file", "Write a file into the local sandbox")
                    .with_parameters(vec![
                        path_param(),
                        ParameterMetadata::required("content", RuninatorType::String),
                    ])
                    .with_results(vec![
                        ResultMetadata::new("path", RuninatorType::String),
                        ResultMetadata::new("size_bytes", RuninatorType::Integer),
                        ResultMetadata::new("location", RuninatorType::String),
                    ]),
                ActionMetadata::new("list_dir", "List a directory in the local sandbox")
                    .with_parameters(vec![path_param()])
                    .with_results(vec![
                        ResultMetadata::new("path", RuninatorType::String),
                        ResultMetadata::new("entries", RuninatorType::array(RuninatorType::Any)),
                        ResultMetadata::new("location", RuninatorType::String),
                    ]),
                ActionMetadata::new("stat", "Stat a path in the local sandbox")
                    .with_parameters(vec![path_param()])
                    .with_results(vec![
                        ResultMetadata::new("path", RuninatorType::String),
                        ResultMetadata::new("exists", RuninatorType::Boolean),
                        ResultMetadata::new("is_dir", RuninatorType::Boolean),
                        ResultMetadata::new("size_bytes", RuninatorType::Integer),
                        ResultMetadata::new("location", RuninatorType::String),
                    ]),
                ActionMetadata::new("delete", "Delete a file in the local sandbox")
                    .with_parameters(vec![path_param()])
                    .with_results(vec![
                        ResultMetadata::new("path", RuninatorType::String),
                        ResultMetadata::new("deleted", RuninatorType::Boolean),
                        ResultMetadata::new("location", RuninatorType::String),
                    ]),
            ],
            metadata: ProviderRuntimeMetadata::default(),
        }
    }

    fn execute_service(
        &self,
        request: ProviderExecutionRequest,
        _sink: Option<Arc<dyn ProviderEventSink>>,
        _token: CancellationToken,
    ) -> Result<TaskExecutionResult, SendableError> {
        execute(&request)
    }
}
