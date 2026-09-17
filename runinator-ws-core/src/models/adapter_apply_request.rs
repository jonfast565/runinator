#[allow(unused_imports)]
use super::*;

#[derive(Debug, Deserialize)]
pub struct AdapterApplyRequest {
    pub name: String,
    pub kind: String,
    pub kind_version: String,
    #[serde(default)]
    pub transport: runinator_models::orchestration::AdapterTransport,
    #[serde(default)]
    pub configuration: Value,
    #[serde(default)]
    pub authentication: runinator_models::orchestration::AdapterAuthentication,
    /// Deprecated compatibility field. New clients send `authentication`.
    #[serde(default)]
    pub secret_bindings: BTreeMap<String, Uuid>,
    #[serde(default)]
    pub identity_configuration: Value,
    #[serde(default)]
    pub expected_revision: Option<i64>,
}

impl Validate for AdapterApplyRequest {
    fn validate(&self) -> Result<(), ValidationError> {
        required_text("name", &self.name, SHORT_TEXT_MAX)?;
        identifier("kind", &self.kind)?;
        identifier("kind_version", &self.kind_version)?;
        if self.expected_revision.is_some_and(|revision| revision < 0) {
            return Err(ValidationError::new(
                "expected_revision",
                "must not be negative",
            ));
        }
        if self.secret_bindings.len() > 128 {
            return Err(ValidationError::new(
                "secret_bindings",
                "must contain at most 128 entries",
            ));
        }
        for key in self.secret_bindings.keys() {
            identifier(&format!("secret_bindings.{key}"), key)?;
        }
        match &self.authentication {
            runinator_models::orchestration::AdapterAuthentication::Secrets { secret_bindings } => {
                if secret_bindings.len() > 128 {
                    return Err(ValidationError::new(
                        "authentication.secret_bindings",
                        "must contain at most 128 entries",
                    ));
                }
                for key in secret_bindings.keys() {
                    identifier(&format!("authentication.secret_bindings.{key}"), key)?;
                }
            }
            runinator_models::orchestration::AdapterAuthentication::ExecutionProfile { .. } => {}
        }
        Ok(())
    }
}
