#[allow(unused_imports)]
use super::*;

/// the resolved principal for an authenticated request, injected as an axum extension.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuthContext {
    pub principal_id: Option<Uuid>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub session_id: Option<Uuid>,
    pub kind: PrincipalKind,
    pub platform_role: Option<PlatformRole>,
    #[serde(default)]
    pub assignments: Vec<RoleAssignment>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub system_role: Option<SystemRole>,
    #[serde(default)]
    pub action_ceiling: Vec<Action>,
    /// active organization for this request, resolved from the token's `org` claim (or an
    /// `X-Org-Id` header for service keys). `None` means platform-global / no org selected.
    pub org_id: Option<Uuid>,
}

impl AuthContext {
    /// The sole administrative authorization override.
    pub fn is_platform_admin(&self) -> bool {
        self.platform_role == Some(PlatformRole::Admin)
    }

    /// the synthetic admin used when auth is disabled, so existing behavior is unchanged.
    pub fn disabled_platform_admin() -> Self {
        Self {
            principal_id: None,
            session_id: None,
            kind: PrincipalKind::Service,
            platform_role: Some(PlatformRole::Admin),
            assignments: Vec::new(),
            system_role: None,
            action_ceiling: Vec::new(),
            org_id: None,
        }
    }
}
