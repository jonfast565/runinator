use runinator_models::providers::ProviderMetadata;
use runinator_plugin::provider::Provider;
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
use runinator_provider_std::StdProvider;

pub type StaticProvider = Box<dyn Provider + Send + Sync>;

/// build the built-in providers used by local runtimes.
pub fn built_in_providers() -> Vec<StaticProvider> {
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
        Box::new(StdProvider {}) as StaticProvider,
    ]
}

/// build the metadata list for the built-in providers.
pub fn metadata() -> Vec<ProviderMetadata> {
    built_in_providers()
        .iter()
        .map(|provider| provider.metadata())
        .collect()
}

#[cfg(test)]
mod tests;
