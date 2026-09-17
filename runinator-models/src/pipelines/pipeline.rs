#[allow(unused_imports)]
use super::*;

/// a named pipeline instance: a chosen set of member workflows plus authoring defaults. the links
/// between members remain `chained` workflow triggers stamped with this pipeline's id; the runtime
/// chaining engine is unaware of pipelines.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Pipeline {
    pub id: Option<Uuid>,
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub key: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub namespace: Option<String>,
    #[serde(default)]
    pub description: Option<String>,
    /// the owning organization (tenant), or `None` for platform-global. stamped from the creator's
    /// active org on create and preserved on update.
    #[serde(default)]
    pub org_id: Option<Uuid>,
    /// Whether this pipeline may admit new manual, trigger, or ingress runs.
    #[serde(default = "default_true")]
    pub enabled: bool,
    #[serde(default)]
    pub graph: PipelineGraph,
    #[serde(default)]
    pub concurrency: WorkflowConcurrency,
    #[serde(default)]
    pub defaults: PipelineDefaults,
    #[serde(default)]
    pub metadata: Value,
    #[serde(default)]
    pub created_at: Option<DateTime<Utc>>,
    #[serde(default)]
    pub updated_at: Option<DateTime<Utc>>,
}

impl Pipeline {
    pub fn artifact_key(&self) -> &str {
        self.key.as_deref().unwrap_or(&self.name)
    }

    pub fn artifact_path(&self) -> crate::artifacts::ArtifactPath {
        crate::artifacts::ArtifactPath::new(self.namespace.clone(), self.artifact_key().to_string())
    }
}

impl crate::validation::Validate for Pipeline {
    fn validate(&self) -> Result<(), crate::validation::ValidationError> {
        use crate::validation::{
            LONG_TEXT_MAX, SHORT_TEXT_MAX, ValidationError, identifier, optional_text,
            required_text, serialized,
        };

        required_text("name", &self.name, SHORT_TEXT_MAX)?;
        if let Some(key) = self.key.as_deref() {
            identifier("key", key)?;
        }
        optional_text("namespace", self.namespace.as_deref(), SHORT_TEXT_MAX)?;
        optional_text("description", self.description.as_deref(), LONG_TEXT_MAX)?;
        if self.concurrency.max_concurrent_runs < 0 {
            return Err(ValidationError::new(
                "concurrency.max_concurrent_runs",
                "must not be negative",
            ));
        }
        for (index, member) in self.graph.members.iter().enumerate() {
            required_text(
                &format!("graph.members[{index}].key"),
                &member.key,
                SHORT_TEXT_MAX,
            )?;
        }
        for (index, link) in self.graph.links.iter().enumerate() {
            required_text(
                &format!("graph.links[{index}].from"),
                &link.from,
                SHORT_TEXT_MAX,
            )?;
            required_text(
                &format!("graph.links[{index}].to"),
                &link.to,
                SHORT_TEXT_MAX,
            )?;
        }
        serialized("pipeline", self)?;
        Ok(())
    }
}
