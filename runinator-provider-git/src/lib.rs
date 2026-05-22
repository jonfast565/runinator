mod command;
mod params;
mod provider;

pub use provider::GitProvider;

#[cfg(test)]
use runinator_models::runs::ProviderExecutionRequest;
#[cfg(test)]
use runinator_plugin::provider::Provider;
#[cfg(test)]
use serde_json::json;

#[cfg(test)]
mod tests;
