#[allow(unused_imports)]
use super::*;

#[derive(Debug, Deserialize)]
pub struct AdapterDraftTestRequest {
    pub draft: AdapterApplyRequest,
    #[serde(default)]
    pub headers: BTreeMap<String, String>,
    #[serde(default)]
    pub body_base64: String,
}

impl Validate for AdapterDraftTestRequest {
    fn validate(&self) -> Result<(), ValidationError> {
        self.draft.validate()?;
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
        bounded_text("body_base64", &self.body_base64, 2 * 1024 * 1024)
    }
}
