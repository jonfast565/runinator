#[allow(unused_imports)]
use super::*;

#[derive(Debug, Deserialize)]
pub struct AdapterTestRequest {
    #[serde(default)]
    pub headers: BTreeMap<String, String>,
    #[serde(default)]
    pub body_base64: String,
    #[serde(default)]
    pub configuration: Option<Value>,
    #[serde(default)]
    pub authentication: Option<runinator_models::orchestration::AdapterAuthentication>,
    /// Deprecated compatibility field. New clients send `authentication`.
    #[serde(default)]
    pub secret_bindings: Option<BTreeMap<String, Uuid>>,
}

impl Validate for AdapterTestRequest {
    fn validate(&self) -> Result<(), ValidationError> {
        if self.headers.len() > 128 {
            return Err(ValidationError::new(
                "headers",
                "must contain at most 128 headers",
            ));
        }
        for (name, value) in &self.headers {
            identifier(&format!("headers.{name}"), name)?;
            if value.contains(['\r', '\n']) {
                return Err(ValidationError::new(
                    format!("headers.{name}"),
                    "must not contain line breaks",
                ));
            }
            bounded_text(&format!("headers.{name}"), value, 8 * 1024)?;
        }
        if self
            .secret_bindings
            .as_ref()
            .is_some_and(|bindings| bindings.len() > 128)
        {
            return Err(ValidationError::new(
                "secret_bindings",
                "must contain at most 128 entries",
            ));
        }
        bounded_text("body_base64", &self.body_base64, 16 * 1024 * 1024)
    }
}
