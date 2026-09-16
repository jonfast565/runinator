mod claude_code;
mod codex;
mod errors;
mod params;
mod provider;
mod shell;
mod usage;

pub use provider::AiCommandProvider;

#[cfg(test)]
use runinator_models::json;
#[cfg(test)]
use runinator_models::runs::ProviderExecutionRequest;
#[cfg(test)]
use runinator_plugin::provider::Provider;

#[cfg(test)]
mod tests;
