//! Email + in-app notification provider.
//!
//! Exposes two actions:
//!   - `email.send`: SMTP delivery via lettre, with credentials read from
//!     the `target_url` (RUNINATOR_HOME-relative is not used; SMTP config
//!     comes via parameters or env: SMTP_HOST/SMTP_PORT/SMTP_USER/SMTP_PASSWORD).
//!   - `notification.send`: Posts a row to `/notifications` on the ws service.
//!     Use this for in-app notifications visible in the Command Center.
//!
//! Both actions persist a `notifications` row when a service URL is reachable
//! (via env var RUNINATOR_SERVICE_URL).

mod params;
mod send;

use std::sync::Arc;

use runinator_models::{
    errors::{RuntimeError, SendableError},
    providers::{
        ActionMetadata, ParameterMetadata, ParameterValueType, ProviderMetadata,
        ProviderRuntimeMetadata, ResultMetadata,
    },
    runs::{ProviderExecutionRequest, TaskExecutionResult},
};
use runinator_plugin::provider::{Provider, ProviderEventSink};

use send::{send_email, send_notification};

#[derive(Clone)]
pub struct EmailProvider;

impl Provider for EmailProvider {
    fn name(&self) -> String {
        "email".into()
    }

    fn metadata(&self) -> ProviderMetadata {
        ProviderMetadata {
            name: self.name(),
            actions: vec![
                ActionMetadata::new("send", "Send an email via SMTP")
                    .with_parameters(vec![
                        ParameterMetadata::required("to", ParameterValueType::String),
                        ParameterMetadata::optional("from", ParameterValueType::String),
                        ParameterMetadata::required("subject", ParameterValueType::String),
                        ParameterMetadata::optional("body", ParameterValueType::String),
                        ParameterMetadata::optional("html_body", ParameterValueType::String),
                        ParameterMetadata::optional("smtp_host", ParameterValueType::String),
                        ParameterMetadata::optional("smtp_port", ParameterValueType::Integer),
                        ParameterMetadata::optional("smtp_user", ParameterValueType::String),
                        ParameterMetadata::optional("smtp_password", ParameterValueType::String),
                    ])
                    .with_results(vec![
                        ResultMetadata::new("sent", ParameterValueType::Boolean),
                        ResultMetadata::new("notification_id", ParameterValueType::Integer),
                        ResultMetadata::new("recipient", ParameterValueType::String),
                    ]),
                ActionMetadata::new(
                    "notify",
                    "Post an in-app notification visible in Command Center",
                )
                .with_parameters(vec![
                    ParameterMetadata::required("title", ParameterValueType::String),
                    ParameterMetadata::optional("body", ParameterValueType::String),
                    ParameterMetadata::optional("severity", ParameterValueType::String),
                    ParameterMetadata::optional("target", ParameterValueType::String),
                    ParameterMetadata::optional("metadata", ParameterValueType::Object),
                ])
                .with_results(vec![
                    ResultMetadata::new("notification_id", ParameterValueType::Integer),
                    ResultMetadata::new("title", ParameterValueType::String),
                ]),
            ],
            metadata: ProviderRuntimeMetadata::default(),
        }
    }

    fn execute_service(
        &self,
        request: ProviderExecutionRequest,
        _sink: Option<Arc<dyn ProviderEventSink>>,
        _token: runinator_plugin::cancel::CancellationToken,
    ) -> Result<TaskExecutionResult, SendableError> {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .map_err(|err| {
                Box::new(RuntimeError::new("email.runtime".into(), err.to_string()))
                    as SendableError
            })?;
        runtime.block_on(async move {
            match request.action_function.as_str() {
                "send" => send_email(&request).await,
                "notify" => send_notification(&request).await,
                other => Err(Box::new(RuntimeError::new(
                    "email.unknown_action".into(),
                    format!("Unknown action {other}"),
                )) as SendableError),
            }
        })
    }
}
