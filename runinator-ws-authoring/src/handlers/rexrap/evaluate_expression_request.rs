#[allow(unused_imports)]
use super::*;

#[derive(Deserialize)]
pub struct EvaluateExpressionRequest {
    #[serde(default)]
    pub expression: Option<Value>,
    #[serde(default)]
    pub source: Option<String>,
    #[serde(default = "default_fragment_kind")]
    pub kind: RexRapFragmentKind,
    #[serde(default)]
    pub context: Value,
}

impl Validate for EvaluateExpressionRequest {
    fn validate(&self) -> Result<(), ValidationError> {
        if let Some(source) = self.source.as_deref() {
            validate_source("source", source)?;
        }
        if self.source.is_none() && self.expression.is_none() {
            return Err(ValidationError::new(
                "expression",
                "expression or source is required",
            ));
        }
        Ok(())
    }
}
