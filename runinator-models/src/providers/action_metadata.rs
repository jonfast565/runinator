#[allow(unused_imports)]
use super::*;

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
