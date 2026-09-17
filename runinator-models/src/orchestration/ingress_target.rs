#[allow(unused_imports)]
use super::*;

/// Stable target identity retained with an admission generation.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct IngressTarget {
    pub kind: IngressTargetKind,
    pub id: Uuid,
}
