#[allow(unused_imports)]
use super::*;

#[derive(Debug, Deserialize)]
pub struct MoveFunctionPackageRequest {
    #[serde(default)]
    pub namespace: Option<String>,
    pub name: String,
}

impl Validate for MoveFunctionPackageRequest {
    fn validate(&self) -> Result<(), ValidationError> {
        identifier("name", &self.name)?;
        if let Some(namespace) = self.namespace.as_deref() {
            identifier("namespace", namespace)?;
        }
        Ok(())
    }
}
