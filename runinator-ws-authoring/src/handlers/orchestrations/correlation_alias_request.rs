#[allow(unused_imports)]
use super::*;

#[derive(Debug, Deserialize)]
pub struct CorrelationAliasRequest {
    pub source: String,
    pub scope: String,
    pub correlation_key: String,
}

impl Validate for CorrelationAliasRequest {
    fn validate(&self) -> Result<(), ValidationError> {
        identifier("source", &self.source)?;
        identifier("scope", &self.scope)?;
        required_text("correlation_key", &self.correlation_key, SHORT_TEXT_MAX)
    }
}
