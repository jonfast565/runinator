use std::collections::HashMap;

use runinator_models::{
    core::ScheduledTask,
    errors::{RuntimeError, SendableError},
};
use runinator_plugin::{plugin::Plugin, provider::Provider};
use runinator_provider_aws::AwsProvider;
use runinator_provider_sql::SqlProvider;

use crate::console_provider::ConsoleProvider;

type StaticProvider = Box<dyn Provider + Send + Sync>;

fn get_providers() -> Vec<StaticProvider> {
    vec![
        Box::new(ConsoleProvider {}) as StaticProvider,
        Box::new(AwsProvider {}) as StaticProvider,
        Box::new(SqlProvider {}) as StaticProvider,
    ]
}

pub fn resolve_provider(
    libraries: &HashMap<String, Plugin>,
    task: &ScheduledTask,
) -> Result<StaticProvider, SendableError> {
    let providers = get_providers();
    if let Some(provider) = providers.into_iter().find(|p| p.name() == task.action_name) {
        return Ok(provider);
    }

    if let Some(plugin) = libraries.get(&task.action_name) {
        return Ok(Box::new(plugin.clone()));
    }

    Err(Box::new(RuntimeError::new(
        "worker.provider.not_found".into(),
        format!("Cannot find plugin/provider {}", task.action_name),
    )))
}
