//! host-side execution contract for compiled-in inbound adapters.
use super::*;
use std::{future::Future, pin::Pin, sync::OnceLock};

type PollFuture = Pin<Box<dyn Future<Output = AdapterPollResponse> + Send>>;

fn validate_configuration(
    metadata: &AdapterKindMetadata,
    request: AdapterValidationRequest,
) -> AdapterValidationResponse {
    use runinator_adapter_contract::{AdapterValidationIssue, AdapterValidationSeverity};
    let mut issues = Vec::new();
    if request.transport == runinator_models::orchestration::AdapterTransport::Polling
        && !metadata.capabilities.iter().any(|value| value == "polling")
    {
        issues.push(AdapterValidationIssue {
            path: "transport".into(),
            code: "unsupported_transport".into(),
            message: "this adapter kind does not support polling".into(),
            severity: AdapterValidationSeverity::Error,
        });
    }
    let fields = if request.transport == runinator_models::orchestration::AdapterTransport::Polling
    {
        metadata.polling_fields.iter().collect::<Vec<_>>()
    } else {
        metadata.fields.iter().collect::<Vec<_>>()
    };
    for field in fields
        .into_iter()
        .filter(|field| field.required && !field.secret)
    {
        let missing = request
            .configuration
            .get(&field.name)
            .is_none_or(|value| value.is_null() || value.as_str().is_some_and(str::is_empty));
        if missing {
            issues.push(AdapterValidationIssue {
                path: format!("configuration.{}", field.name),
                code: "required".into(),
                message: format!("{} is required", field.name),
                severity: AdapterValidationSeverity::Error,
            });
        }
    }
    if let Some(value) = request.configuration.get("poll_interval_seconds")
        && !value.is_null()
        && !value
            .as_i64()
            .is_some_and(|value| (30..=3_600).contains(&value))
    {
        issues.push(AdapterValidationIssue {
            path: "configuration.poll_interval_seconds".into(),
            code: "out_of_range".into(),
            message: "poll interval must be an integer between 30 and 3600 seconds".into(),
            severity: AdapterValidationSeverity::Error,
        });
    }
    if metadata.kind == "github"
        && request.transport == runinator_models::orchestration::AdapterTransport::Polling
        && !request
            .configuration
            .get("repositories")
            .and_then(Value::as_array)
            .is_some_and(|items| {
                !items.is_empty()
                    && items.iter().all(|item| {
                        item.as_str().is_some_and(|value| {
                            value.split_once('/').is_some_and(|(owner, repository)| {
                                !owner.is_empty() && !repository.is_empty()
                            })
                        })
                    })
            })
    {
        issues.push(AdapterValidationIssue {
            path: "configuration.repositories".into(),
            code: "invalid_repository".into(),
            message: "repositories must use owner/name form".into(),
            severity: AdapterValidationSeverity::Error,
        });
    }
    if metadata.kind == "jira"
        && request
            .configuration
            .get("routing_scope")
            .and_then(Value::as_str)
            .is_some_and(|scope| scope.trim() == "mission.sdlc")
        && !request
            .configuration
            .get("sdlc_profile")
            .is_some_and(Value::is_object)
    {
        issues.push(AdapterValidationIssue {
            path: "configuration.sdlc_profile".into(),
            code: "required_for_sdlc".into(),
            message: "an SDLC project profile is required when routing to mission.sdlc".into(),
            severity: AdapterValidationSeverity::Error,
        });
    }
    AdapterValidationResponse { issues }
}

pub(super) fn registry() -> &'static BTreeMap<String, Box<dyn BuiltinAdapter>> {
    static REGISTRY: OnceLock<BTreeMap<String, Box<dyn BuiltinAdapter>>> = OnceLock::new();
    REGISTRY.get_or_init(|| {
        let adapters: Vec<Box<dyn BuiltinAdapter>> = vec![
            Box::new(GenericWebhook),
            Box::new(Github),
            Box::new(Jira),
            Box::new(SlackIngress),
        ];
        adapters
            .into_iter()
            .map(|adapter| (adapter.metadata().kind, adapter))
            .collect()
    })
}
pub(super) fn unsupported_poll(request: AdapterPollRequest) -> AdapterPollResponse {
    AdapterPollResponse {
host_version: None,
kind_version: None,
        events: Vec::new(),
        checkpoint: request.checkpoint,
        retry_after_seconds: None,
        error: Some("adapter kind does not support polling".into()),
    }
}

#[cfg(test)]
#[path = "builtins_tests.rs"]
mod tests;

mod builtin_adapter;
pub(super) use builtin_adapter::BuiltinAdapter;

mod generic_webhook;
use generic_webhook::GenericWebhook;

mod github;
use github::Github;

mod jira;
use jira::Jira;

mod slack_ingress;
use slack_ingress::SlackIngress;
