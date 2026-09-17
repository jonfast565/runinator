#[allow(unused_imports)]
use super::*;

/// one export in a publish request.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct NewFunctionExport {
    pub name: String,
    /// the entry point inside the package, e.g. `src.images.resize`.
    pub handler: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(default)]
    pub input: Vec<ParameterMetadata>,
    #[serde(default)]
    pub output: Vec<ResultMetadata>,
    #[serde(default)]
    pub limits: FunctionResourceLimits,
}

impl NewFunctionExport {
    pub(super) fn validate_at(&self, index: usize) -> Result<(), ValidationError> {
        let path = format!("exports[{index}]");
        identifier(&format!("{path}.name"), &self.name)?;
        required_text(&format!("{path}.handler"), &self.handler, SHORT_TEXT_MAX)?;
        optional_text(
            &format!("{path}.description"),
            self.description.as_deref(),
            LONG_TEXT_MAX,
        )?;
        for (field, value, max) in [
            ("timeout_seconds", self.limits.timeout_seconds, 86_400),
            ("memory_mb", self.limits.memory_mb, 1_048_576),
            ("cpu_millis", self.limits.cpu_millis, 1_000_000),
            ("pids", self.limits.pids, 65_536),
            ("tmp_mb", self.limits.tmp_mb, 1_048_576),
        ] {
            if !(1..=max).contains(&value) {
                return Err(ValidationError::new(
                    format!("{path}.limits.{field}"),
                    format!("must be between 1 and {max}"),
                ));
            }
        }
        Ok(())
    }
}
