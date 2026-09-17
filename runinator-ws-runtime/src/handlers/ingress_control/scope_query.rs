#[allow(unused_imports)]
use super::*;

#[derive(Debug, Deserialize)]
pub struct ScopeQuery {
    pub(super) scope_kind: ScopeKind,
    pub(super) scope_id: Option<Uuid>,
}
