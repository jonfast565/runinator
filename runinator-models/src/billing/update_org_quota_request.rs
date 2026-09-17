#[allow(unused_imports)]
use super::*;

/// platform-admin quota update.
#[derive(Debug, Clone, Deserialize)]
pub struct UpdateOrgQuotaRequest {
    #[serde(default)]
    pub max_nodes_per_kind: BTreeMap<String, u32>,
    #[serde(default)]
    pub max_monthly_cents: u32,
}

impl Validate for UpdateOrgQuotaRequest {
    fn validate(&self) -> Result<(), ValidationError> {
        if self.max_nodes_per_kind.len() > 32 {
            return Err(ValidationError::new(
                "max_nodes_per_kind",
                "must contain at most 32 replica kinds",
            ));
        }
        for key in self.max_nodes_per_kind.keys() {
            identifier(&format!("max_nodes_per_kind.{key}"), key)?;
        }
        Ok(())
    }
}
