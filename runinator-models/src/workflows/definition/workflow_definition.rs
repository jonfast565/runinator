#[allow(unused_imports)]
use super::*;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(remote = "Self")]
pub struct WorkflowDefinition {
    pub id: Option<Uuid>,
    pub name: String,
    /// Stable authoring key for this logical workflow. Display-name edits and namespace moves do
    /// not change it. Older definitions omit it and temporarily fall back to `name` until the
    /// namespace migration writes an explicit key.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub key: Option<String>,
    /// the namespace that qualifies this workflow's identity, from a `namespace <path>` header.
    /// `None` for an unqualified workflow. a subflow target `"<namespace>.<name>"` resolves against
    /// the qualified identity `namespace + "." + name`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub namespace: Option<String>,
    /// the organization (tenant) that owns this workflow. `None` means platform-global / unassigned,
    /// which keeps pre-tenancy workflows working unchanged.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub org_id: Option<Uuid>,
    #[serde(default)]
    pub version: SemVer,
    #[serde(default)]
    pub enabled: bool,
    #[serde(default)]
    #[serde(deserialize_with = "deserialize_workflow_type")]
    pub input_type: RuninatorType,
    /// Published return contract; omitted declarations remain unknown (`Any`).
    #[serde(default)]
    pub output_type: RuninatorType,
    #[serde(default)]
    pub definition: WorkflowGraph,
    #[serde(default)]
    pub created_at: Option<DateTime<Utc>>,
    #[serde(default)]
    pub updated_at: Option<DateTime<Utc>>,
}

impl Serialize for WorkflowDefinition {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        Self::serialize(self, serializer)
    }
}

impl<'de> Deserialize<'de> for WorkflowDefinition {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let value = serde_json::Value::deserialize(deserializer)?;
        Self::deserialize(crate::workflow_contracts::with_legacy_output_type(value))
            .map_err(serde::de::Error::custom)
    }
}

impl WorkflowDefinition {
    /// The durable key used when source does not carry the UUID directly.
    pub fn artifact_key(&self) -> &str {
        self.key.as_deref().unwrap_or(&self.name)
    }

    /// The current human-facing path. This is an alias for the UUID, not the artifact identity.
    pub fn artifact_path(&self) -> crate::artifacts::ArtifactPath {
        crate::artifacts::ArtifactPath::new(self.namespace.clone(), self.artifact_key().to_string())
    }
}

impl crate::validation::Validate for WorkflowDefinition {
    fn validate(&self) -> Result<(), crate::validation::ValidationError> {
        use crate::validation::{
            SHORT_TEXT_MAX, identifier, optional_text, required_text, serialized,
        };

        required_text("name", &self.name, SHORT_TEXT_MAX)?;
        if let Some(key) = self.key.as_deref() {
            identifier("key", key)?;
        }
        optional_text("namespace", self.namespace.as_deref(), SHORT_TEXT_MAX)?;
        serialized("workflow", self)?;
        Ok(())
    }
}
