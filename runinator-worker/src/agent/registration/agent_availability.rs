#[allow(unused_imports)]
use super::*;

#[derive(Clone)]
pub(crate) struct AgentAvailability {
    pub(super) instance_id: String,
    pub(super) display_name: Option<String>,
    pub(super) host: Option<String>,
    pub(super) version: Option<String>,
    pub(super) attributes: Value,
    pub(super) providers: crate::provider_repository::ProviderFactory,
    pub(super) publish_providers: bool,
}

impl AgentAvailability {
    pub(crate) fn from_config(config: &AgentRuntimeConfig) -> Self {
        Self {
            instance_id: config.instance_id.clone(),
            display_name: config.display_name.clone(),
            host: config.advertise_host.clone(),
            version: config.version.clone(),
            attributes: registration_attributes(config),
            providers: Arc::clone(&config.providers),
            publish_providers: config.publish_providers,
        }
    }
}
