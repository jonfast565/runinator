#[allow(unused_imports)]
use super::*;

#[derive(Deserialize)]
pub struct RexRapSourceRequest {
    pub source: String,
    #[serde(default)]
    pub fragment: Option<RexRapFragmentKind>,
    #[serde(default)]
    pub document: RexRapDocumentKind,
}

impl Validate for RexRapSourceRequest {
    fn validate(&self) -> Result<(), ValidationError> {
        validate_source("source", &self.source)
    }
}
