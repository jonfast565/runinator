#[allow(unused_imports)]
use super::*;

/// A live UI hint sent to every web-service replica so connected WebSocket clients can refetch.
/// Delivery is best effort. A dropped event only leaves a panel briefly stale.
/// Each replica can drop the event at WebSocket egress when [`Self::org_id`] does not match the
/// caller's active organization.
///
/// wire shape keeps the historical tagged `type` field via flatten, with an optional sibling
/// `org_id`. older publishers that omit `org_id` deserialize as unscoped (`None`) and remain
/// visible to every client during the rollout phase.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UiEvent {
    /// When set, WS egress delivers only to platform admins and clients in the active organization.
    /// When absent, every connected client can see the event.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub org_id: Option<Uuid>,
    /// Exact owner scope for new event kinds. `org_id` remains populated where available for
    /// mixed-version compatibility, while egress authorization prefers this field.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub scope: Option<runinator_models::rbac::ScopeRef>,
    #[serde(flatten)]
    pub kind: UiEventKind,
}

impl UiEvent {
    pub fn new(org_id: Option<Uuid>, kind: UiEventKind) -> Self {
        let scope = org_id.and_then(|id| {
            runinator_models::rbac::ScopeRef::new(
                runinator_models::rbac::ScopeKind::Organization,
                Some(id),
            )
        });
        Self {
            org_id,
            scope,
            kind,
        }
    }

    /// unscoped / platform-global hint.
    pub fn global(kind: UiEventKind) -> Self {
        Self::new(None, kind)
    }

    pub fn for_org(org_id: Uuid, kind: UiEventKind) -> Self {
        Self::new(Some(org_id), kind)
    }

    pub fn for_scope(scope: runinator_models::rbac::ScopeRef, kind: UiEventKind) -> Self {
        let org_id = if scope.kind == runinator_models::rbac::ScopeKind::Organization {
            scope.id
        } else {
            None
        };
        Self {
            org_id,
            scope: Some(scope),
            kind,
        }
    }
}
