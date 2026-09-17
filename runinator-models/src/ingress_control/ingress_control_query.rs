#[allow(unused_imports)]
use super::*;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IngressControlQuery {
    pub scope: Option<ScopeRef>,
    pub target_kind: Option<IngressTargetKind>,
    pub target_id: Option<Uuid>,
    pub state: Option<IngressControlState>,
    pub limit: i64,
}
