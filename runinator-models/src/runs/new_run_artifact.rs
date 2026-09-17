#[allow(unused_imports)]
use super::*;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NewRunArtifact {
    pub name: String,
    pub mime_type: String,
    pub size_bytes: i64,
    pub uri: String,
    #[serde(default)]
    pub metadata: Value,
}

impl From<ProviderExecutionEvent> for Option<NewRunArtifact> {
    fn from(event: ProviderExecutionEvent) -> Self {
        match event {
            ProviderExecutionEvent::Artifact {
                name,
                mime_type,
                size_bytes,
                uri,
                metadata,
            } => Some(NewRunArtifact {
                name,
                mime_type,
                size_bytes,
                uri,
                metadata,
            }),
            _ => None,
        }
    }
}
