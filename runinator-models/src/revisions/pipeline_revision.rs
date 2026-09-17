#[allow(unused_imports)]
use super::*;

/// One immutable pipeline definition snapshot. The mutable `pipelines` row is the current head;
/// this record supplies exact pinning, integrity checks, history, and rollback material.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PipelineRevision {
    pub id: Uuid,
    pub pipeline_id: Uuid,
    pub revision: i64,
    pub digest: String,
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(default)]
    pub graph: PipelineGraph,
    #[serde(default)]
    pub concurrency: WorkflowConcurrency,
    #[serde(default)]
    pub defaults: PipelineDefaults,
    #[serde(default)]
    pub metadata: Value,
    pub source: RevisionSource,
    #[serde(default)]
    pub actor_id: Option<Uuid>,
    pub actor_kind: String,
    #[serde(default)]
    pub note: Option<String>,
    pub created_at: Option<DateTime<Utc>>,
}

impl PipelineRevision {
    pub fn content_digest(
        graph: &PipelineGraph,
        concurrency: &WorkflowConcurrency,
        defaults: &PipelineDefaults,
        metadata: &Value,
    ) -> String {
        let payload = serde_json::to_vec(&(graph, concurrency, defaults, metadata))
            .expect("pipeline revision payload is serializable");
        let mut digest = Sha256::new();
        digest.update(payload);
        format!("sha256:{}", hex::encode(digest.finalize()))
    }

    pub fn from_pipeline(pipeline: &Pipeline, author: &RevisionAuthor) -> Option<Self> {
        let pipeline_id = pipeline.id?;
        Some(Self {
            id: Uuid::nil(),
            pipeline_id,
            revision: 0,
            digest: Self::content_digest(
                &pipeline.graph,
                &pipeline.concurrency,
                &pipeline.defaults,
                &pipeline.metadata,
            ),
            name: pipeline.name.clone(),
            description: pipeline.description.clone(),
            graph: pipeline.graph.clone(),
            concurrency: pipeline.concurrency,
            defaults: pipeline.defaults.clone(),
            metadata: pipeline.metadata.clone(),
            source: author.source,
            actor_id: author.actor_id,
            actor_kind: author.actor_kind.clone(),
            note: author.note.clone(),
            created_at: None,
        })
    }

    pub fn to_pipeline(&self, current: &Pipeline) -> Pipeline {
        Pipeline {
            id: current.id,
            name: self.name.clone(),
            key: current.key.clone(),
            namespace: current.namespace.clone(),
            description: self.description.clone(),
            org_id: current.org_id,
            enabled: current.enabled,
            graph: self.graph.clone(),
            concurrency: self.concurrency,
            defaults: self.defaults.clone(),
            metadata: self.metadata.clone(),
            created_at: current.created_at,
            updated_at: None,
        }
    }
}
