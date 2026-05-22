mod claude_code;
mod params;
mod provider;
mod shell;

pub use provider::AiCommandProvider;

#[cfg(test)]
use runinator_models::runs::ProviderExecutionRequest;
#[cfg(test)]
use runinator_plugin::provider::Provider;
#[cfg(test)]
use serde_json::json;

#[cfg(test)]
mod tests;
