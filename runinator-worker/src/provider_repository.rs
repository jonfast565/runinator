use std::collections::HashMap;
use std::sync::Arc;

use runinator_models::{errors::SendableError, workflows::WorkflowAction};
use runinator_plugin::plugin::Plugin;
use runinator_provider_catalog::{StaticProvider, built_in_providers};

// builds the provider set a worker resolves against. invoked fresh per resolution so each provider
// instance is owned by the action that executes it. the default yields the shared built-in catalog;
// an embedded worker (e.g. the desktop) supplies its own set.
pub type ProviderFactory = Arc<dyn Fn() -> Vec<StaticProvider> + Send + Sync>;

/// the default provider set: the shared built-in catalog.
pub fn default_provider_factory() -> ProviderFactory {
    Arc::new(built_in_providers)
}

pub fn resolve_provider(
    providers: &ProviderFactory,
    libraries: &HashMap<String, Plugin>,
    action: &WorkflowAction,
) -> Result<StaticProvider, SendableError> {
    if let Some(provider) = providers()
        .into_iter()
        .find(|p| p.name() == action.provider)
    {
        return Ok(provider);
    }

    if let Some(plugin) = libraries.get(&action.provider) {
        tracing::debug!(provider = %action.provider, "resolved provider from a loaded plugin");
        return Ok(Box::new(plugin.clone()));
    }

    tracing::warn!(
        provider = %action.provider,
        loaded_plugins = libraries.len(),
        "provider not found among built-ins or loaded plugins"
    );
    Err(crate::errors::PROVIDER_NOT_FOUND.error(&action.provider))
}
