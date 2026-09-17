#[allow(unused_imports)]
use super::*;

pub trait AuthContextExt {
    fn is_platform_admin(&self) -> bool;
    fn selected_scope(&self) -> ScopeRef;
    fn authorize_scope(&self, action: Action, scope: ScopeRef) -> bool;
    fn require_scope_action(
        &self,
        action: Action,
        scope: ScopeRef,
    ) -> Result<(), AuthorizationDenied>;
    fn require_system_role(&self, roles: &[SystemRole]) -> Result<(), AuthorizationDenied>;
    fn actor_kind(&self) -> &'static str;
    fn revision_author(&self) -> RevisionAuthor;
}

impl AuthContextExt for AuthContext {
    fn is_platform_admin(&self) -> bool {
        AuthContext::is_platform_admin(self)
    }

    fn selected_scope(&self) -> ScopeRef {
        self.org_id
            .and_then(|id| ScopeRef::new(ScopeKind::Organization, Some(id)))
            .unwrap_or(ScopeRef::PLATFORM)
    }

    fn authorize_scope(&self, action: Action, scope: ScopeRef) -> bool {
        if !self.action_ceiling.is_empty() && !self.action_ceiling.contains(&action) {
            return false;
        }
        if self.is_platform_admin() {
            return true;
        }
        if scope.kind == ScopeKind::User
            && scope.id == self.principal_id
            && user_owner_allows(action)
        {
            return true;
        }
        if self
            .platform_role
            .is_some_and(|role| role_allows(Role::Platform(role), action))
        {
            return true;
        }
        matches!(scope.kind, ScopeKind::Organization | ScopeKind::Team)
            && self
                .assignments
                .iter()
                .any(|assignment| assignment.scope == scope && role_allows(assignment.role, action))
    }

    fn require_scope_action(
        &self,
        action: Action,
        scope: ScopeRef,
    ) -> Result<(), AuthorizationDenied> {
        self.authorize_scope(action, scope)
            .then_some(())
            .ok_or_else(AuthorizationDenied::forbidden)
    }

    fn require_system_role(&self, roles: &[SystemRole]) -> Result<(), AuthorizationDenied> {
        let allowed = roles.iter().copied().any(|role| {
            (self.is_platform_admin() || self.system_role == Some(role))
                && (self.action_ceiling.is_empty()
                    || self.action_ceiling.contains(&system_role_action(role)))
        });
        if allowed {
            Ok(())
        } else {
            Err(AuthorizationDenied::forbidden())
        }
    }

    /// the audit `actor_kind` string for a principal.
    fn actor_kind(&self) -> &'static str {
        match self.kind {
            PrincipalKind::User => "user",
            PrincipalKind::Service => "service",
        }
    }

    /// describe the caller as the author of a definition write, for the revision history.
    ///
    /// The source is inferred from the principal kind. A user token is classified as `UI`, and a
    /// service key is classified as `API`. This is only a hint: a person using curl still gets the
    /// `UI` label. The import path records whether the write came from a pack or a hand edit.
    fn revision_author(&self) -> RevisionAuthor {
        RevisionAuthor {
            contract_override_reason: None,
            actor_id: self.principal_id,
            actor_kind: self.actor_kind().to_string(),
            source: match self.kind {
                PrincipalKind::User => RevisionSource::Ui,
                PrincipalKind::Service => RevisionSource::Api,
            },
            note: None,
        }
    }
}
