#[allow(unused_imports)]
use super::*;

#[derive(Debug, Deserialize)]
pub struct ControlQuery {
    pub(super) scope_kind: Option<ScopeKind>,
    pub(super) scope_id: Option<Uuid>,
    pub(super) target_kind: Option<IngressTargetKind>,
    pub(super) target_id: Option<Uuid>,
    pub(super) state: Option<IngressControlState>,
    pub(super) limit: Option<i64>,
}
