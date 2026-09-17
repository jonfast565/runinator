#[allow(unused_imports)]
use super::*;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct ScopeRef {
    pub kind: ScopeKind,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub id: Option<Uuid>,
}

impl ScopeRef {
    pub const PLATFORM: Self = Self {
        kind: ScopeKind::Platform,
        id: None,
    };

    pub fn new(kind: ScopeKind, id: Option<Uuid>) -> Option<Self> {
        if matches!(kind, ScopeKind::Platform) == id.is_none() {
            Some(Self { kind, id })
        } else {
            None
        }
    }
}
