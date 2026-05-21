use std::collections::{BTreeMap, HashMap};

use runinator_models::{
    errors::{RuntimeError, SendableError},
    providers::ProviderMetadata,
    workflows::WorkflowAction,
};
use runinator_plugin::{plugin::Plugin, provider::Provider};
use runinator_provider_ai::AiCommandProvider;
use runinator_provider_approval::ApprovalProvider;
use runinator_provider_aws::AwsProvider;
use runinator_provider_git::GitProvider;
use runinator_provider_github::GitHubProvider;
use runinator_provider_jira::JiraProvider;
use runinator_provider_slack::SlackProvider;
use runinator_provider_sql::SqlProvider;

use crate::console_provider::ConsoleProvider;

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
    ]
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

pub fn provider_metadata(libraries: &HashMap<String, Plugin>) -> Vec<ProviderMetadata> {
    let mut providers = get_providers()
        .into_iter()
        .map(|provider| {
            let metadata = provider.metadata();
            (metadata.name.clone(), metadata)
        })
        .collect::<BTreeMap<_, _>>();

    for plugin in libraries.values() {
        if providers.contains_key(&plugin.name) {
            continue;
        }
        let metadata = plugin.metadata();
        providers.entry(metadata.name.clone()).or_insert(metadata);
    }

    providers.into_values().collect()
}

#[cfg(test)]
#[path = "provider_repository/tests.rs"]
mod tests;
