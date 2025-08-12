use std::collections::HashMap;

use runinator_models::{
    core::ScheduledTask,
    errors::{RuntimeError, SendableError},
};
use runinator_plugin::{plugin::Plugin, provider::Provider};
use runinator_provider_aws::AwsProvider;
use runinator_provider_sql::SqlProvider;

pub(crate) type StaticProvider = Box<dyn Provider + Send + Sync>;

pub(crate) fn get_providers() -> Vec<StaticProvider> {
    let mut result = Vec::new();
    result.push(Box::new(AwsProvider {}) as StaticProvider);
    result.push(Box::new(SqlProvider {}) as StaticProvider);
    result
}

pub(crate) async fn get_plugin_or_provider(
    libraries: &HashMap<String, Plugin>,
    task: &ScheduledTask,
) -> Result<StaticProvider, SendableError> {
    if let Some(plugin) = libraries.get(&task.action_name) {
        return Ok(Box::new(plugin.clone()));
    }

    let providers = get_providers();
    let match_provider: Option<StaticProvider> = providers
        .into_iter()
        .find(|provider| provider.name() == task.action_name)
        .map(|provider| provider);

    if let Some(provider) = match_provider {
        return Ok(provider);
    }

    Err(Box::new(RuntimeError::new(
        "2".to_string(),
        format!("Cannot find plugin/provider {}", task.action_name),
    )))
}
