use serde::{Deserialize, Serialize};

use crate::orchestration::DeliverySemantics;
use crate::types::RuninatorField;
pub use crate::types::RuninatorType;
use crate::value::{Map, Value};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ProviderMetadata {
    pub name: String,
    #[serde(default)]
    pub actions: Vec<ActionMetadata>,
    #[serde(default)]
    pub metadata: ProviderRuntimeMetadata,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct ProviderRuntimeMetadata {
    #[serde(default)]
    pub credential_scopes: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub contract: Option<String>,
    /// How this provider safely consumes an effect-private execution profile.
    #[serde(default)]
    pub execution_profile: ExecutionProfileSupport,
}

#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ExecutionProfileSupport {
    #[default]
    Unsupported,
    Subprocess,
    InProcess,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ActionMetadata {
    pub function_name: String,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub parameters: Vec<ParameterMetadata>,
    #[serde(default)]
    pub results: Vec<ResultMetadata>,
    /// whether this library function is pure (reducer-evaluable in-process) or effectful
    /// (worker-only). defaults to false so existing providers stay effectful.
    #[serde(default)]
    pub pure: bool,
    /// Delivery contract used when an effect is scoped to a correlated orchestration binding.
    #[serde(default)]
    pub delivery_semantics: DeliverySemantics,
    /// Optional authoring hints for actions that drive an autonomous mission phase.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub agent: Option<AgentActionMetadata>,
    /// Credential alternatives accepted by this action. Missing metadata preserves the legacy
    /// behavior where secret parameters and execution profiles are validated independently.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub authentication: Option<ActionAuthenticationMetadata>,
    /// Credential scopes required when this action uses an execution profile. Omission preserves
    /// the legacy provider-level scope contract.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub credential_scopes: Option<Vec<String>>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ActionAuthenticationMetadata {
    #[serde(default = "default_true")]
    pub required: bool,
    pub alternatives: Vec<ActionAuthenticationAlternative>,
    /// Whether more than one satisfied alternative may be supplied. Later credential injection
    /// takes precedence when two alternatives target the same subprocess setting.
    #[serde(default, skip_serializing_if = "is_false")]
    pub allow_multiple: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ActionAuthenticationAlternative {
    Secrets { parameters: Vec<String> },
    ExecutionProfile,
}

impl ActionAuthenticationMetadata {
    pub fn required(alternatives: Vec<ActionAuthenticationAlternative>) -> Self {
        Self {
            required: true,
            alternatives,
            allow_multiple: false,
        }
    }

    pub fn optional(alternatives: Vec<ActionAuthenticationAlternative>) -> Self {
        Self {
            required: false,
            alternatives,
            allow_multiple: false,
        }
    }

    pub fn allow_multiple(mut self) -> Self {
        self.allow_multiple = true;
        self
    }
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

/// Catalog-declared semantics used by mission authoring without coupling clients to a provider.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct AgentActionMetadata {
    pub prompt_parameter: String,
    pub response_text_pointer: String,
}

impl ActionMetadata {
    pub fn new(function_name: impl Into<String>, description: impl Into<String>) -> Self {
        Self {
            function_name: function_name.into(),
            description: Some(description.into()),
            parameters: Vec::new(),
            results: Vec::new(),
            pure: false,
            delivery_semantics: DeliverySemantics::AtLeastOnce,
            agent: None,
            authentication: None,
            credential_scopes: None,
        }
    }

    pub fn with_parameters(mut self, parameters: Vec<ParameterMetadata>) -> Self {
        self.parameters = parameters;
        self
    }

    pub fn with_results(mut self, results: Vec<ResultMetadata>) -> Self {
        self.results = results;
        self
    }

    pub fn as_agent(
        mut self,
        prompt_parameter: impl Into<String>,
        response_text_pointer: impl Into<String>,
    ) -> Self {
        self.agent = Some(AgentActionMetadata {
            prompt_parameter: prompt_parameter.into(),
            response_text_pointer: response_text_pointer.into(),
        });
        self
    }

    pub fn with_authentication(mut self, authentication: ActionAuthenticationMetadata) -> Self {
        self.authentication = Some(authentication);
        self
    }

    pub fn with_credential_scopes(
        mut self,
        scopes: impl IntoIterator<Item = impl Into<String>>,
    ) -> Self {
        self.credential_scopes = Some(scopes.into_iter().map(Into::into).collect());
        self
    }

    /// the typed parameter environment for this action.
    pub fn parameters_type(&self) -> RuninatorType {
        RuninatorType::typed_structure(self.parameters.iter().map(|parameter| {
            let field = if parameter.required {
                RuninatorField::required(parameter.ty.clone())
            } else {
                RuninatorField::optional(parameter.ty.clone())
            };
            let field = match &parameter.default_value {
                Some(default_value) => field.with_default(default_value.clone()),
                None => field,
            };
            (parameter.name.clone(), field)
        }))
    }

    /// the typed result environment for this action.
    pub fn results_type(&self) -> RuninatorType {
        RuninatorType::typed_structure(self.results.iter().map(|result| {
            (
                result.name.clone(),
                RuninatorField::optional(result.ty.clone()),
            )
        }))
    }

    /// mark this function as pure (reducer-evaluable in-process).
    pub fn pure(mut self) -> Self {
        self.pure = true;
        self
    }

    pub fn with_delivery_semantics(mut self, semantics: DeliverySemantics) -> Self {
        self.delivery_semantics = semantics;
        self
    }

    pub fn to_json_schema(&self) -> Value {
        let mut properties = Map::new();
        let mut required = Vec::new();
        for param in &self.parameters {
            let mut prop = param.ty.to_json_schema();
            if let Some(desc) = &param.description
                && let Value::Object(object) = &mut prop
            {
                object.insert("description".into(), Value::String(desc.clone()));
            }
            properties.insert(param.name.clone(), prop);
            if param.required {
                required.push(Value::String(param.name.clone()));
            }
        }
        crate::json!({
            "type": "object",
            "properties": Value::Object(properties),
            "required": required,
        })
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ParameterMetadata {
    pub name: String,
    // `type` is accepted as well as `ty`: these schemas are hand-written in function
    // manifests, and `ty` is a rust field name rather than something an author would reach for.
    // serialization still emits `ty`, so nothing downstream sees a second spelling.
    #[serde(alias = "type", deserialize_with = "deserialize_type")]
    pub ty: RuninatorType,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(default)]
    pub required: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub default_value: Option<Value>,
    #[serde(default)]
    pub secret: bool,
    /// Worker-side destinations populated from this secret parameter after late resolution.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub credential_injections: Vec<CredentialInjection>,
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

impl ParameterMetadata {
    pub fn required(name: impl Into<String>, ty: RuninatorType) -> Self {
        Self {
            name: name.into(),
            ty,
            label: None,
            description: None,
            required: true,
            default_value: None,
            secret: false,
            credential_injections: Vec::new(),
        }
    }

    pub fn optional(name: impl Into<String>, ty: RuninatorType) -> Self {
        Self {
            required: false,
            ..Self::required(name, ty)
        }
    }

    pub fn with_description(mut self, description: impl Into<String>) -> Self {
        self.description = Some(description.into());
        self
    }

    pub fn secret(mut self) -> Self {
        self.secret = true;
        self
    }

    pub fn inject(mut self, injection: CredentialInjection) -> Self {
        self.credential_injections.push(injection);
        self
    }

    pub fn with_default(mut self, default_value: impl Into<Value>) -> Self {
        self.default_value = Some(default_value.into());
        self
    }
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

impl crate::validation::Validate for ProviderMetadata {
    fn validate(&self) -> Result<(), crate::validation::ValidationError> {
        use crate::validation::{
            LONG_TEXT_MAX, SHORT_TEXT_MAX, ValidationError, identifier, optional_text, serialized,
        };

        identifier("name", &self.name)?;
        for (index, scope) in self.metadata.credential_scopes.iter().enumerate() {
            identifier(&format!("metadata.credential_scopes[{index}]"), scope)?;
        }
        optional_text(
            "metadata.contract",
            self.metadata.contract.as_deref(),
            SHORT_TEXT_MAX,
        )?;
        for (action_index, action) in self.actions.iter().enumerate() {
            identifier(
                &format!("actions[{action_index}].function_name"),
                &action.function_name,
            )?;
            optional_text(
                &format!("actions[{action_index}].description"),
                action.description.as_deref(),
                LONG_TEXT_MAX,
            )?;
            if let Some(scopes) = &action.credential_scopes {
                for (scope_index, scope) in scopes.iter().enumerate() {
                    identifier(
                        &format!("actions[{action_index}].credential_scopes[{scope_index}]"),
                        scope,
                    )?;
                }
            }
            for (parameter_index, parameter) in action.parameters.iter().enumerate() {
                identifier(
                    &format!("actions[{action_index}].parameters[{parameter_index}].name"),
                    &parameter.name,
                )?;
                optional_text(
                    &format!("actions[{action_index}].parameters[{parameter_index}].label"),
                    parameter.label.as_deref(),
                    SHORT_TEXT_MAX,
                )?;
                optional_text(
                    &format!("actions[{action_index}].parameters[{parameter_index}].description"),
                    parameter.description.as_deref(),
                    LONG_TEXT_MAX,
                )?;
            }
            for (result_index, result) in action.results.iter().enumerate() {
                identifier(
                    &format!("actions[{action_index}].results[{result_index}].name"),
                    &result.name,
                )?;
                optional_text(
                    &format!("actions[{action_index}].results[{result_index}].label"),
                    result.label.as_deref(),
                    SHORT_TEXT_MAX,
                )?;
                optional_text(
                    &format!("actions[{action_index}].results[{result_index}].description"),
                    result.description.as_deref(),
                    LONG_TEXT_MAX,
                )?;
            }
        }
        validate_provider_metadata(self)
            .map_err(|message| ValidationError::new("provider", message))?;
        serialized("provider", self)?;
        Ok(())
    }
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

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ResultMetadata {
    pub name: String,
    // `type` is accepted as well as `ty`: these schemas are hand-written in function
    // manifests, and `ty` is a rust field name rather than something an author would reach for.
    // serialization still emits `ty`, so nothing downstream sees a second spelling.
    #[serde(alias = "type", deserialize_with = "deserialize_type")]
    pub ty: RuninatorType,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
}

impl ResultMetadata {
    pub fn new(name: impl Into<String>, ty: RuninatorType) -> Self {
        Self {
            name: name.into(),
            ty,
            label: None,
            description: None,
        }
    }

    pub fn with_description(mut self, description: impl Into<String>) -> Self {
        self.description = Some(description.into());
        self
    }

    pub fn with_type(mut self, ty: RuninatorType) -> Self {
        self.ty = ty;
        self
    }
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
