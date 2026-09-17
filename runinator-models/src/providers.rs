use serde::{Deserialize, Serialize};

use crate::orchestration::DeliverySemantics;
use crate::types::RuninatorField;
pub use crate::types::RuninatorType;
use crate::value::{Map, Value};

#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ExecutionProfileSupport {
    #[default]
    Unsupported,
    Subprocess,
    InProcess,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ActionAuthenticationAlternative {
    Secrets { parameters: Vec<String> },
    ExecutionProfile,
}

impl ActionAuthenticationAlternative {
    pub fn secrets(parameters: impl IntoIterator<Item = impl Into<String>>) -> Self {
        Self::Secrets {
            parameters: parameters.into_iter().map(Into::into).collect(),
        }
    }
}

const fn default_true() -> bool {
    true
}

const fn is_false(value: &bool) -> bool {
    !*value
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum CredentialInjection {
    Parameter {
        name: String,
        #[serde(default = "default_secret_template")]
        template: String,
    },
    Environment {
        name: String,
        #[serde(default = "default_secret_template")]
        template: String,
    },
    Arguments {
        values: Vec<String>,
    },
    Header {
        name: String,
        #[serde(default = "default_secret_template")]
        template: String,
    },
}

fn default_secret_template() -> String {
    "${secret}".into()
}

pub fn validate_provider_metadata(metadata: &ProviderMetadata) -> Result<(), String> {
    if metadata.name.trim().is_empty() {
        return Err("provider name is required".into());
    }
    for action in &metadata.actions {
        validate_action_metadata(metadata, action)?;
    }
    Ok(())
}

/// Validate one action's selected credential alternative against its resolved or authored input.
pub fn validate_action_authentication(
    metadata: &ActionMetadata,
    parameters: &Value,
    has_execution_profile: bool,
) -> Result<(), String> {
    let Some(authentication) = &metadata.authentication else {
        return Ok(());
    };
    let object = parameters.as_object();
    let mut selected = 0usize;
    for alternative in &authentication.alternatives {
        let satisfied = match alternative {
            ActionAuthenticationAlternative::Secrets { parameters } => {
                let present = parameters
                    .iter()
                    .filter(|name| {
                        object
                            .and_then(|object| object.get(name))
                            .is_some_and(|value| !blank_credential_value(value))
                    })
                    .count();
                if present > 0 && present < parameters.len() {
                    return Err(format!(
                        "secret authentication is incomplete; provide {}",
                        parameters.join(", ")
                    ));
                }
                present == parameters.len()
            }
            ActionAuthenticationAlternative::ExecutionProfile => has_execution_profile,
        };
        selected += usize::from(satisfied);
    }
    if selected > 1 && !authentication.allow_multiple {
        return Err("select exactly one authentication method; secrets and an execution profile cannot be combined".into());
    }
    if authentication.required && selected == 0 {
        return Err(
            "select a required stored-secret or execution-profile authentication method".into(),
        );
    }
    Ok(())
}

fn blank_credential_value(value: &Value) -> bool {
    value.is_null() || value.as_str().is_some_and(|value| value.trim().is_empty())
}

fn validate_action_metadata(
    provider: &ProviderMetadata,
    action: &ActionMetadata,
) -> Result<(), String> {
    if action.function_name.trim().is_empty() {
        return Err(format!(
            "provider '{}' has an action without a function name",
            provider.name
        ));
    }
    let mut names = std::collections::BTreeSet::new();
    let mut injection_targets = std::collections::BTreeSet::new();
    for parameter in &action.parameters {
        if parameter.name.trim().is_empty() {
            return Err(format!(
                "provider '{}.{}' has a parameter without a name",
                provider.name, action.function_name
            ));
        }
        if !names.insert(parameter.name.as_str()) {
            return Err(format!(
                "provider '{}.{}' has duplicate parameter '{}'",
                provider.name, action.function_name, parameter.name
            ));
        }
        if !parameter.credential_injections.is_empty()
            && (!parameter.secret || parameter.ty != RuninatorType::String)
        {
            return Err(format!(
                "provider '{}.{}' parameter '{}' may inject credentials only when it is a secret string",
                provider.name, action.function_name, parameter.name
            ));
        }
        for injection in &parameter.credential_injections {
            validate_credential_injection(
                provider,
                action,
                parameter,
                injection,
                &mut injection_targets,
            )?;
        }
        let Some(default_value) = &parameter.default_value else {
            continue;
        };

        parameter
            .ty
            .validate_value(default_value)
            .map_err(|violation| {
                violation.message_with_label(&format!(
                    "provider '{}.{}' parameter '{}'",
                    provider.name, action.function_name, parameter.name
                ))
            })?;
    }
    if let Some(authentication) = &action.authentication {
        if authentication.alternatives.is_empty() {
            return Err(format!(
                "provider '{}.{}' authentication must declare at least one alternative",
                provider.name, action.function_name
            ));
        }
        let mut alternatives = std::collections::BTreeSet::new();
        for alternative in &authentication.alternatives {
            let key = serde_json::to_string(alternative).map_err(|error| error.to_string())?;
            if !alternatives.insert(key) {
                return Err(format!(
                    "provider '{}.{}' has a duplicate authentication alternative",
                    provider.name, action.function_name
                ));
            }
            match alternative {
                ActionAuthenticationAlternative::Secrets { parameters } => {
                    if parameters.is_empty() {
                        return Err(format!(
                            "provider '{}.{}' has an empty secret authentication alternative",
                            provider.name, action.function_name
                        ));
                    }
                    let mut secret_names = std::collections::BTreeSet::new();
                    for name in parameters {
                        let parameter = action.parameters.iter().find(|item| item.name == *name);
                        if !secret_names.insert(name) {
                            return Err(format!(
                                "provider '{}.{}' repeats secret parameter '{}' in one authentication alternative",
                                provider.name, action.function_name, name
                            ));
                        }
                        if !parameter
                            .is_some_and(|item| item.secret && item.ty == RuninatorType::String)
                        {
                            return Err(format!(
                                "provider '{}.{}' authentication references non-secret string parameter '{}'",
                                provider.name, action.function_name, name
                            ));
                        }
                    }
                }
                ActionAuthenticationAlternative::ExecutionProfile => {
                    if provider.metadata.execution_profile == ExecutionProfileSupport::Unsupported {
                        return Err(format!(
                            "provider '{}.{}' requires an execution profile but the provider does not support one",
                            provider.name, action.function_name
                        ));
                    }
                }
            }
        }
    }
    Ok(())
}

fn validate_credential_injection(
    provider: &ProviderMetadata,
    action: &ActionMetadata,
    parameter: &ParameterMetadata,
    injection: &CredentialInjection,
    targets: &mut std::collections::BTreeSet<String>,
) -> Result<(), String> {
    let (target, templates) = match injection {
        CredentialInjection::Parameter { name, template } => {
            if !action
                .parameters
                .iter()
                .any(|candidate| candidate.name == *name && candidate.ty == RuninatorType::String)
            {
                return Err(format!(
                    "provider '{}.{}' credential parameter target '{}' is not a declared string parameter",
                    provider.name, action.function_name, name
                ));
            }
            (format!("parameter:{name}"), vec![template.as_str()])
        }
        CredentialInjection::Environment { name, template } => {
            if !crate::execution_profiles::is_portable_environment_name(name) {
                return Err(format!(
                    "provider '{}.{}' credential environment target '{}' is invalid",
                    provider.name, action.function_name, name
                ));
            }
            (
                format!("environment:{}", name.to_ascii_uppercase()),
                vec![template.as_str()],
            )
        }
        CredentialInjection::Arguments { values } => {
            if values.is_empty() || values.iter().any(|value| value.is_empty()) {
                return Err(format!(
                    "provider '{}.{}' credential arguments for '{}' must be nonempty",
                    provider.name, action.function_name, parameter.name
                ));
            }
            (
                format!("arguments:{}", parameter.name),
                values.iter().map(String::as_str).collect(),
            )
        }
        CredentialInjection::Header { name, template } => {
            if !valid_header_name(name) {
                return Err(format!(
                    "provider '{}.{}' credential header target '{}' is invalid",
                    provider.name, action.function_name, name
                ));
            }
            (
                format!("header:{}", name.to_ascii_lowercase()),
                vec![template.as_str()],
            )
        }
    };
    if !targets.insert(target) {
        return Err(format!(
            "provider '{}.{}' has duplicate credential injection targets",
            provider.name, action.function_name
        ));
    }
    let placeholder_count = templates
        .iter()
        .map(|template| template.matches("${secret}").count())
        .sum::<usize>();
    if placeholder_count != 1 {
        return Err(format!(
            "provider '{}.{}' credential injection for '{}' must contain exactly one '${{secret}}' placeholder",
            provider.name, action.function_name, parameter.name
        ));
    }
    Ok(())
}

fn valid_header_name(name: &str) -> bool {
    !name.is_empty()
        && name.bytes().all(|byte| {
            byte.is_ascii_alphanumeric()
                || matches!(
                    byte,
                    b'!' | b'#'
                        | b'$'
                        | b'%'
                        | b'&'
                        | b'\''
                        | b'*'
                        | b'+'
                        | b'-'
                        | b'.'
                        | b'^'
                        | b'_'
                        | b'`'
                        | b'|'
                        | b'~'
                )
        })
}

fn deserialize_type<'de, D>(deserializer: D) -> Result<RuninatorType, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let value = Value::deserialize(deserializer)?;
    if let Some(raw) = value.as_str() {
        return Ok(match raw {
            "string" => RuninatorType::String,
            "integer" => RuninatorType::Integer,
            "number" => RuninatorType::Number,
            "boolean" => RuninatorType::Boolean,
            "string_array" => RuninatorType::array(RuninatorType::String),
            "number_array" => RuninatorType::array(RuninatorType::Number),
            "object" => RuninatorType::map(RuninatorType::Any),
            "json" => RuninatorType::Any,
            other => {
                return Err(serde::de::Error::custom(format!(
                    "unknown legacy value type '{other}'"
                )));
            }
        });
    }
    serde_json::from_value(value.into()).map_err(serde::de::Error::custom)
}

mod provider_metadata;
pub use provider_metadata::ProviderMetadata;

mod provider_runtime_metadata;
pub use provider_runtime_metadata::ProviderRuntimeMetadata;

mod action_metadata;
pub use action_metadata::ActionMetadata;

mod action_authentication_metadata;
pub use action_authentication_metadata::ActionAuthenticationMetadata;

mod agent_action_metadata;
pub use agent_action_metadata::AgentActionMetadata;

mod parameter_metadata;
pub use parameter_metadata::ParameterMetadata;

mod result_metadata;
pub use result_metadata::ResultMetadata;
