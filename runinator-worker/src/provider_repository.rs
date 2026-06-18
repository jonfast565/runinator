use std::collections::HashMap;

use runinator_models::{errors::SendableError, workflows::WorkflowAction};
use runinator_plugin::plugin::Plugin;
use runinator_provider_catalog::{StaticProvider, built_in_providers};

pub fn resolve_provider(
    libraries: &HashMap<String, Plugin>,
    action: &WorkflowAction,
) -> Result<StaticProvider, SendableError> {
    let providers = built_in_providers();
    if let Some(provider) = providers.into_iter().find(|p| p.name() == action.provider) {
        return Ok(provider);
    }

    if let Some(plugin) = libraries.get(&action.provider) {
        return Ok(Box::new(plugin.clone()));
    }

    Err(crate::errors::PROVIDER_NOT_FOUND.error(&action.provider))
}
