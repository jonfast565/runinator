#[allow(unused_imports)]
use super::*;

/// a request to publish one version of a package.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct NewFunctionVersion {
    pub package: NewFunctionPackage,
    pub artifact_digest: String,
    /// the parsed manifest, kept verbatim so a republish can be compared against what was published.
    #[serde(default)]
    pub manifest: Value,
    pub runtime: FunctionRuntimeSpec,
    pub exports: Vec<NewFunctionExport>,
    /// move this alias onto the new version once it is published.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub alias: Option<String>,
}

impl Validate for NewFunctionVersion {
    fn validate(&self) -> Result<(), ValidationError> {
        self.package.validate()?;
        if !is_valid_digest(&self.artifact_digest) {
            return Err(ValidationError::new(
                "artifact_digest",
                "must be a sha256: digest with 64 hexadecimal characters",
            ));
        }
        identifier("runtime.runtime", &self.runtime.runtime)?;
        optional_text("runtime.image", self.runtime.image.as_deref(), 2 * 1024)?;
        optional_text(
            "runtime.setup_script",
            self.runtime.setup_script.as_deref(),
            LONG_TEXT_MAX,
        )?;
        optional_text("alias", self.alias.as_deref(), SHORT_TEXT_MAX)?;
        if self.exports.is_empty() {
            return Err(ValidationError::new(
                "exports",
                "must contain at least one export",
            ));
        }
        if self.exports.len() > 256 {
            return Err(ValidationError::new(
                "exports",
                "must contain at most 256 exports",
            ));
        }
        for (index, export) in self.exports.iter().enumerate() {
            export.validate_at(index)?;
        }
        Ok(())
    }
}
