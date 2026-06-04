use std::collections::HashMap;

use runinator_models::{
    bundles::ProviderBundle,
    errors::{RuntimeError, SendableError},
    workflows::WorkflowAction,
};
use runinator_plugin::{plugin::Plugin, provider::Provider};
use runinator_provider_ai::AiCommandProvider;
use runinator_provider_approval::ApprovalProvider;
use runinator_provider_aws::AwsProvider;
use runinator_provider_console::ConsoleProvider;
use runinator_provider_email::EmailProvider;
use runinator_provider_git::GitProvider;
use runinator_provider_github::GitHubProvider;
use runinator_provider_jira::JiraProvider;
use runinator_provider_slack::SlackProvider;
use runinator_provider_sql::SqlProvider;

type StaticProvider = Box<dyn Provider + Send + Sync>;

fn get_providers() -> Vec<StaticProvider> {
    vec![
        Box::new(ConsoleProvider {}) as StaticProvider,
        Box::new(AwsProvider {}) as StaticProvider,
        Box::new(SqlProvider {}) as StaticProvider,
        Box::new(JiraProvider {}) as StaticProvider,
        Box::new(GitHubProvider {}) as StaticProvider,
        Box::new(SlackProvider {}) as StaticProvider,
        Box::new(GitProvider {}) as StaticProvider,
        Box::new(AiCommandProvider {}) as StaticProvider,
        Box::new(ApprovalProvider {}) as StaticProvider,
        Box::new(EmailProvider {}) as StaticProvider,
    ]
}

/// build the metadata bundle for the built-in providers so the worker can register them.
pub fn metadata_bundle() -> ProviderBundle {
    ProviderBundle {
        providers: get_providers().iter().map(|p| p.metadata()).collect(),
    }
}

pub fn resolve_provider(
    libraries: &HashMap<String, Plugin>,
    action: &WorkflowAction,
) -> Result<StaticProvider, SendableError> {
    let providers = get_providers();
    if let Some(provider) = providers.into_iter().find(|p| p.name() == action.provider) {
        return Ok(provider);
    }

    if let Some(plugin) = libraries.get(&action.provider) {
        return Ok(Box::new(plugin.clone()));
    }

    Err(Box::new(RuntimeError::new(
        "worker.provider.not_found".into(),
        format!("Cannot find plugin/provider {}", action.provider),
    )))
}
