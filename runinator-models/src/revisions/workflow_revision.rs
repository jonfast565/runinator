#[allow(unused_imports)]
use super::*;

/// one immutable capture of a workflow definition.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(remote = "Self")]
pub struct WorkflowRevision {
    pub id: Uuid,
    pub workflow_id: Uuid,
    /// monotonic per workflow, 1-based. `revision` is the stable handle a rollback names.
    pub revision: i64,
    /// SHA-256 of the executable revision payload. A revision pin records this alongside its
    /// sequence number so a corrupted or substituted snapshot is never mistaken for the one an
    /// author selected.
    #[serde(default)]
    pub digest: String,
    pub version: SemVer,
    pub name: String,
    #[serde(default)]
    pub input_type: RuninatorType,
    #[serde(default)]
    pub output_type: RuninatorType,
    #[serde(default)]
    pub definition: WorkflowGraph,
    pub source: RevisionSource,
    #[serde(default)]
    pub actor_id: Option<Uuid>,
    pub actor_kind: String,
    #[serde(default)]
    pub note: Option<String>,
    pub created_at: Option<DateTime<Utc>>,
}

impl Serialize for WorkflowRevision {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        Self::serialize(self, serializer)
    }
}

impl<'de> Deserialize<'de> for WorkflowRevision {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let value = serde_json::Value::deserialize(deserializer)?;
        Self::deserialize(crate::workflow_contracts::with_legacy_output_type(value))
            .map_err(serde::de::Error::custom)
    }
}

impl WorkflowRevision {
    /// Includes the published output without changing the legacy digest algorithm.
    pub fn contract_digest(
        version: SemVer,
        input_type: &RuninatorType,
        output_type: &RuninatorType,
        definition: &WorkflowGraph,
    ) -> String {
        let payload = serde_json::to_vec(&(
            "workflow-contract-v2",
            version,
            input_type,
            output_type,
            definition,
        ))
        .expect("workflow revision payload is serializable");
        format!("sha256:{}", hex::encode(Sha256::digest(payload)))
    }

    /// Canonical digest of the parts of a revision that affect execution. Namespace and display
    /// name intentionally do not participate: they are mutable aliases of the logical workflow,
    /// not part of an immutable workflow definition.
    pub fn content_digest(
        version: SemVer,
        input_type: &RuninatorType,
        definition: &WorkflowGraph,
    ) -> String {
        let payload = serde_json::to_vec(&(version, input_type, definition))
            .expect("workflow revision payload is serializable");
        let mut digest = Sha256::new();
        digest.update(payload);
        format!("sha256:{}", hex::encode(digest.finalize()))
    }

    /// rebuild a savable definition from this revision, carrying the *current* row's identity
    /// (id/namespace/org/enabled) so restoring never re-tenants or re-enables a workflow.
    pub fn to_definition(&self, current: &WorkflowDefinition) -> WorkflowDefinition {
        WorkflowDefinition {
            id: current.id,
            name: self.name.clone(),
            key: current.key.clone(),
            namespace: current.namespace.clone(),
            org_id: current.org_id,
            version: self.version,
            enabled: current.enabled,
            input_type: self.input_type.clone(),
            output_type: self.output_type.clone(),
            definition: self.definition.clone(),
            created_at: current.created_at,
            updated_at: None,
        }
    }
}
