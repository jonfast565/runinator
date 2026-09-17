#[allow(unused_imports)]
use super::*;

/// what a caller sends to move an alias.
#[derive(Debug, Deserialize)]
pub struct SetFunctionAliasRequest {
    pub alias: String,
    /// the version to point at, by number. omitted means the newest version.
    #[serde(default)]
    pub version: Option<i64>,
    /// or by another alias, so `production` can be pointed at whatever `latest` currently is.
    #[serde(default)]
    pub from_alias: Option<String>,
}

impl Validate for SetFunctionAliasRequest {
    fn validate(&self) -> Result<(), ValidationError> {
        identifier("alias", &self.alias)?;
        if self.version.is_some_and(|version| version <= 0) {
            return Err(ValidationError::new("version", "must be greater than zero"));
        }
        if let Some(alias) = self.from_alias.as_deref() {
            identifier("from_alias", alias)?;
        }
        if self.version.is_some() && self.from_alias.is_some() {
            return Err(ValidationError::new(
                "from_alias",
                "cannot be combined with version",
            ));
        }
        Ok(())
    }
}
