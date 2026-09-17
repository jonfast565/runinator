#[allow(unused_imports)]
use super::*;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NewRunChunk {
    pub stream: String,
    pub content: String,
}

impl From<ProviderExecutionEvent> for Option<NewRunChunk> {
    fn from(event: ProviderExecutionEvent) -> Self {
        match event {
            ProviderExecutionEvent::Chunk { stream, content } => {
                Some(NewRunChunk { stream, content })
            }
            _ => None,
        }
    }
}
