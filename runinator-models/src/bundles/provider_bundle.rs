#[allow(unused_imports)]
use super::*;

#[derive(Debug, Clone, Default, Serialize, serde::Deserialize)]
pub struct ProviderBundle {
    #[serde(default)]
    pub providers: Vec<ProviderMetadata>,
}

impl Bundle for ProviderBundle {
    const RESOURCE: &'static str = "/providers/import";
}

impl crate::validation::Validate for ProviderBundle {
    fn validate(&self) -> Result<(), crate::validation::ValidationError> {
        for (index, provider) in self.providers.iter().enumerate() {
            crate::validation::Validate::validate(provider).map_err(|error| {
                crate::validation::ValidationError::new(
                    format!("providers[{index}].{}", error.path),
                    error.message,
                )
            })?;
        }
        crate::validation::serialized("provider_bundle", self)
    }
}
