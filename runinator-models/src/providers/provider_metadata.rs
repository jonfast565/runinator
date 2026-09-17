#[allow(unused_imports)]
use super::*;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ProviderMetadata {
    pub name: String,
    #[serde(default)]
    pub actions: Vec<ActionMetadata>,
    #[serde(default)]
    pub metadata: ProviderRuntimeMetadata,
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
