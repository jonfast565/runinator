#[allow(unused_imports)]
use super::*;

/// the package half of a publish, which is upserted rather than required to exist.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct NewFunctionPackage {
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub namespace: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub org_id: Option<Uuid>,
}

impl Validate for NewFunctionPackage {
    fn validate(&self) -> Result<(), ValidationError> {
        identifier("package.name", &self.name)?;
        if let Some(namespace) = self.namespace.as_deref() {
            identifier("package.namespace", namespace)?;
        }
        optional_text(
            "package.description",
            self.description.as_deref(),
            LONG_TEXT_MAX,
        )
    }
}
