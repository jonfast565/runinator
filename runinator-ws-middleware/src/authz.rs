//! Deny-by-default hierarchical authorization helpers.

use std::collections::HashSet;

use axum::{Json, http::StatusCode};
use chrono::Utc;
use runinator_models::auth::{
    AuthContext, Grant, Permission, PrincipalKind, PrincipalType, ResourceType,
};
use runinator_models::errors::error_code_or_unknown;
use runinator_models::orgs::OrgRole;
use runinator_models::rbac::{
    Action, PlatformRole, Role, ScopeKind, ScopeRef, SystemRole, TeamRole,
};
use runinator_models::revisions::{RevisionAuthor, RevisionSource};
use runinator_models::value::Value;
use runinator_store::{
    RuntimeStore,
    roles::{AuthStore, AutomationStore, RbacStore, ScheduleStore},
};
use uuid::Uuid;

use runinator_ws_core::models::{ApiError, ApiResponse};

type Reply = (StatusCode, Json<ApiResponse>);

/// Whether one owned resource may consume another reusable resource. This is evaluated again at
/// run admission so an ownership transfer or revoked grant applies to future runs.
pub async fn resource_can_consume<T: AuthorizationStore>(
    db: &T,
    consumer_type: ResourceType,
    consumer_id: Uuid,
    dependency_type: ResourceType,
    dependency_id: Uuid,
) -> Result<bool, runinator_models::errors::SendableError> {
    runinator_store::resource_access::resource_can_consume(
        db,
        consumer_type,
        consumer_id,
        dependency_type,
        dependency_id,
    )
    .await
}

/// Persistence needed to make one authorization decision, including parent-resource lookups and
/// its best-effort denial audit. This is deliberately narrower than the full database surface:
/// authorization does not need workflow definitions, credentials settings, functions, or task
/// history.
pub trait AuthorizationStore:
    AuthStore + RbacStore + ScheduleStore + RuntimeStore + AutomationStore
{
}

impl<T> AuthorizationStore for T where
    T: AuthStore + RbacStore + ScheduleStore + RuntimeStore + AutomationStore
{
}

fn forbidden() -> Reply {
    (
        StatusCode::FORBIDDEN,
        Json(ApiResponse::ApiError(ApiError::new(
            "insufficient permission for this resource",
        ))),
    )
}

fn not_found() -> Reply {
    (
        StatusCode::NOT_FOUND,
        Json(ApiResponse::ApiError(ApiError::new("resource not found"))),
    )
}

fn authorization_error() -> Reply {
    (
        StatusCode::INTERNAL_SERVER_ERROR,
        Json(ApiResponse::ApiError(ApiError::new(
            "authorization state could not be resolved",
        ))),
    )
}

/// pure predicates over an [`AuthContext`] with no store access. a local trait since `AuthContext`
/// lives in `runinator-models`, which stays free of WS-layer response concepts.
pub trait AuthContextExt {
    fn is_platform_admin(&self) -> bool;
    fn selected_scope(&self) -> ScopeRef;
    fn authorize_scope(&self, action: Action, scope: ScopeRef) -> bool;
    fn require_scope_action(&self, action: Action, scope: ScopeRef) -> Result<(), Reply>;
    fn require_system_role(&self, roles: &[SystemRole]) -> Result<(), Reply>;
    fn actor_kind(&self) -> &'static str;
    fn revision_author(&self) -> RevisionAuthor;
}

impl AuthContextExt for AuthContext {
    fn is_platform_admin(&self) -> bool {
        self.platform_role == Some(PlatformRole::Admin)
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
        let local_role = match scope.kind {
            ScopeKind::Platform => None,
            ScopeKind::User if scope.id == self.principal_id => {
                if user_owner_allows(action) {
                    return true;
                }
                None
            }
            ScopeKind::Organization | ScopeKind::Team => self
                .assignments
                .iter()
                .filter(|assignment| assignment.scope == scope)
                .map(|assignment| assignment.role)
                .max_by_key(|role| role_rank(*role)),
            _ => None,
        };
        let role = self
            .platform_role
            .map(Role::Platform)
            .into_iter()
            .chain(local_role)
            .max_by_key(|role| role_rank(*role));
        role.is_some_and(|role| role_allows(role, action))
    }

    fn require_scope_action(&self, action: Action, scope: ScopeRef) -> Result<(), Reply> {
        self.authorize_scope(action, scope)
            .then_some(())
            .ok_or_else(forbidden)
    }

    fn require_system_role(&self, roles: &[SystemRole]) -> Result<(), Reply> {
        let assigned = self.system_role.filter(|role| roles.contains(role));
        if !self.is_platform_admin() && assigned.is_none() {
            return Err(forbidden());
        }
        let ceiling_allows = self.action_ceiling.is_empty()
            || assigned
                .map(system_role_action)
                .into_iter()
                .chain(roles.iter().copied().map(system_role_action))
                .any(|action| self.action_ceiling.contains(&action));
        if ceiling_allows {
            Ok(())
        } else {
            Err(forbidden())
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

fn system_role_action(role: SystemRole) -> Action {
    match role {
        SystemRole::Engine => Action::EngineOperate,
        SystemRole::Worker => Action::WorkerOperate,
        SystemRole::Waker => Action::WakerOperate,
        SystemRole::Agent => Action::AgentOperate,
        SystemRole::Replica => Action::ReplicaOperate,
    }
}

fn role_rank(role: Role) -> u8 {
    match role {
        Role::Platform(PlatformRole::Member) => 1,
        Role::Platform(PlatformRole::Auditor) => 2,
        Role::Platform(PlatformRole::Operator) => 3,
        Role::Platform(PlatformRole::Admin) => 4,
        Role::Organization(OrgRole::Member) => 1,
        Role::Organization(OrgRole::Operator) => 2,
        Role::Organization(OrgRole::Admin) => 3,
        Role::Organization(OrgRole::Owner) => 4,
        Role::Team(TeamRole::Member) => 1,
        Role::Team(TeamRole::Operator) => 2,
        Role::Team(TeamRole::Admin) => 3,
        Role::Team(TeamRole::Owner) => 4,
        Role::System(_) => 0,
    }
}

fn user_owner_allows(action: Action) -> bool {
    matches!(
        action,
        Action::View | Action::Run | Action::Edit | Action::Own | Action::CredentialsManage
    )
}

fn role_allows(role: Role, action: Action) -> bool {
    match role {
        Role::Platform(PlatformRole::Admin) => true,
        Role::Platform(PlatformRole::Operator) => !matches!(
            action,
            Action::Own
                | Action::RolesManage
                | Action::MembersManage
                | Action::CredentialsManage
                | Action::SecretsRead
                | Action::SecretsWrite
                | Action::BillingManage
        ),
        Role::Platform(PlatformRole::Auditor) => matches!(
            action,
            Action::View | Action::AuditRead | Action::DeadLettersRead
        ),
        Role::Platform(PlatformRole::Member) => action == Action::View,
        Role::Organization(OrgRole::Owner) => true,
        Role::Organization(OrgRole::Admin) => action != Action::Own,
        Role::Organization(OrgRole::Operator) => matches!(
            action,
            Action::View
                | Action::Run
                | Action::Edit
                | Action::NodesOperate
                | Action::SchedulesManage
                | Action::NotificationsManage
                | Action::FunctionsManage
                | Action::ConsoleUse
                | Action::CatalogManage
        ),
        Role::Organization(OrgRole::Member) => action == Action::View,
        Role::Team(TeamRole::Owner) => true,
        Role::Team(TeamRole::Admin) => action != Action::Own,
        Role::Team(TeamRole::Operator) => matches!(
            action,
            Action::View
                | Action::Run
                | Action::Edit
                | Action::SchedulesManage
                | Action::NotificationsManage
                | Action::FunctionsManage
                | Action::ConsoleUse
        ),
        Role::Team(TeamRole::Member) => action == Action::View,
        Role::System(_) => false,
    }
}

fn scope_permission(ctx: &AuthContext, scope: ScopeRef) -> Option<Permission> {
    if scope.kind == ScopeKind::User && scope.id == ctx.principal_id {
        return Some(Permission::Own);
    }
    (scope.kind == ScopeKind::Platform)
        .then_some(ctx.platform_role)
        .flatten()
        .map(Role::Platform)
        .into_iter()
        .chain(
            ctx.assignments
                .iter()
                .filter(|assignment| assignment.scope == scope)
                .map(|assignment| assignment.role),
        )
        .map(Role::default_permission)
        .max()
}

fn permission_action(permission: Permission) -> Action {
    match permission {
        Permission::View => Action::View,
        Permission::Run => Action::Run,
        Permission::Edit => Action::Edit,
        Permission::Own => Action::Own,
    }
}

fn ceiling_allows(ctx: &AuthContext, permission: Permission) -> bool {
    ctx.action_ceiling.is_empty() || ctx.action_ceiling.contains(&permission_action(permission))
}

/// resource-visibility checks that need both a store handle and the caller's identity. `db` and
/// `ctx` are genuinely invariant across every method here (unlike a graph cursor's node/run, which
/// varies per call), so both belong on `self` rather than threaded through each call individually.
pub struct AuthzChecker<'a, T: AuthorizationStore> {
    pub db: &'a T,
    pub ctx: &'a AuthContext,
}

impl<'a, T: AuthorizationStore> AuthzChecker<'a, T> {
    pub fn new(db: &'a T, ctx: &'a AuthContext) -> Self {
        Self { db, ctx }
    }

    /// Require a permission on any ACL-backed top-level resource. Callers must resolve child
    /// identifiers to one of these authoritative parents before invoking this method.
    pub async fn require_resource(
        &self,
        resource_type: ResourceType,
        resource_id: Uuid,
        needed: Permission,
    ) -> Result<(), Reply> {
        if !ceiling_allows(self.ctx, needed) {
            return Err(forbidden());
        }
        match self.resource_permission(resource_type, resource_id).await {
            Err(reply) => Err(reply),
            Ok(Some(permission)) if permission.allows(needed) => Ok(()),
            Ok(None) => {
                self.audit_resource_denied(resource_type, resource_id, needed)
                    .await;
                Err(not_found())
            }
            Ok(Some(_)) => {
                self.audit_resource_denied(resource_type, resource_id, needed)
                    .await;
                Err(forbidden())
            }
        }
    }

    /// the caller's effective permission on a workflow, or `None` when they have no access.
    pub async fn workflow_permission(&self, workflow_id: Uuid) -> Option<Permission> {
        self.resource_permission(ResourceType::Workflow, workflow_id)
            .await
            .ok()
            .flatten()
    }

    /// require at least `needed` permission on the workflow, else a 403 reply.
    pub async fn require_workflow(
        &self,
        workflow_id: Uuid,
        needed: Permission,
    ) -> Result<(), Reply> {
        self.require_resource(ResourceType::Workflow, workflow_id, needed)
            .await
    }

    /// record an authorization denial against a workflow resource.
    async fn audit_resource_denied(
        &self,
        resource_type: ResourceType,
        resource_id: Uuid,
        needed: Permission,
    ) {
        // Authorization is the middleware's own durable concern. Keep this best-effort audit
        // write on the `RuntimeStore` contract instead of reaching into the engine solely for a
        // compatibility helper.
        let record = runinator_models::json!({
            "actor_id": self.ctx.principal_id.map(|id| id.to_string()),
            "actor_kind": self.ctx.actor_kind(),
            "action": "authz.denied",
            "outcome": "denied",
            "resource_type": resource_type.as_str(),
            "resource_id": resource_id.to_string(),
            "detail": format!("missing {:?} permission", needed),
        });
        if let Err(err) = self.db.record_audit_log(record).await {
            log::error!(
                "failed to persist authz.denied audit log (error code {}): {err}",
                error_code_or_unknown(err.as_ref())
            );
        }
    }

    /// the workflow ids the caller can see, or `None` meaning "all" (admin / auth disabled).
    pub async fn visible_workflow_ids(&self) -> Result<Option<HashSet<Uuid>>, Reply> {
        self.visible_resource_ids(ResourceType::Workflow).await
    }

    pub async fn visible_resource_ids(
        &self,
        resource_type: ResourceType,
    ) -> Result<Option<HashSet<Uuid>>, Reply> {
        if self.ctx.is_platform_admin() && ceiling_allows(self.ctx, Permission::View) {
            return Ok(None);
        }
        let mut ids = HashSet::new();
        let ownerships = self
            .db
            .list_resource_ownerships(resource_type)
            .await
            .map_err(|_| authorization_error())?;
        for ownership in ownerships {
            if self
                .resource_permission(resource_type, ownership.resource_id)
                .await?
                .is_some_and(|permission| permission.allows(Permission::View))
            {
                ids.insert(ownership.resource_id);
            }
        }
        Ok(Some(ids))
    }

    /// stamp the creator as `own` on a freshly created workflow. a no-op for service/admin
    /// principals without a user id (nothing to own it).
    pub async fn grant_owner(&self, workflow_id: Uuid) -> Result<(), Reply> {
        self.grant_resource_owner(ResourceType::Workflow, workflow_id)
            .await
    }

    /// Register a newly-created top-level resource under the caller's selected tenant. A platform
    /// resource is always platform-owned; a human creator receives a direct `own` grant instead
    /// of turning the global resource into a user-owned one.
    pub async fn grant_resource_owner(
        &self,
        resource_type: ResourceType,
        resource_id: Uuid,
    ) -> Result<(), Reply> {
        let tenant = self
            .ctx
            .org_id
            .and_then(|id| ScopeRef::new(ScopeKind::Organization, Some(id)))
            .unwrap_or(ScopeRef::PLATFORM);
        let owner = if tenant.kind == ScopeKind::Platform {
            ScopeRef::PLATFORM
        } else {
            match (self.ctx.kind, self.ctx.principal_id) {
                (PrincipalKind::User, Some(id)) => {
                    ScopeRef::new(ScopeKind::User, Some(id)).unwrap()
                }
                _ => tenant,
            }
        };
        let now = Utc::now();
        self.db
            .put_resource_ownership(runinator_models::rbac::ResourceOwnership {
                resource_type,
                resource_id,
                tenant,
                owner,
                created_by: self.ctx.principal_id,
                authz_version: 1,
                created_at: now,
                updated_at: now,
            })
            .await
            .map_err(|_| authorization_error())?;
        let Some(user_id) = self.ctx.principal_id else {
            return Ok(());
        };
        let grant = Grant {
            id: None,
            resource_type,
            resource_id,
            principal_type: PrincipalType::User,
            principal_id: user_id,
            permission: Permission::Own,
            created_at: Utc::now(),
        };
        self.db
            .create_grant(grant)
            .await
            .map_err(|_| authorization_error())?;
        Ok(())
    }

    /// the caller's effective permission on a pipeline, or `None` when they have no access.
    pub async fn pipeline_permission(&self, pipeline_id: Uuid) -> Option<Permission> {
        self.resource_permission(ResourceType::Pipeline, pipeline_id)
            .await
            .ok()
            .flatten()
    }

    /// require at least `needed` permission on the pipeline, else a 403 reply.
    pub async fn require_pipeline(
        &self,
        pipeline_id: Uuid,
        needed: Permission,
    ) -> Result<(), Reply> {
        self.require_resource(ResourceType::Pipeline, pipeline_id, needed)
            .await
    }

    pub async fn resource_permission(
        &self,
        resource_type: ResourceType,
        resource_id: Uuid,
    ) -> Result<Option<Permission>, Reply> {
        if self.ctx.is_platform_admin() {
            return Ok(Some(Permission::Own));
        }
        let ownership = self
            .db
            .fetch_resource_ownership(resource_type, resource_id)
            .await
            .map_err(|_| authorization_error())?;
        let Some(ownership) = ownership else {
            return Ok(None);
        };
        if ownership.tenant.kind == ScopeKind::Organization
            && !self.ctx.authorize_scope(Action::View, ownership.tenant)
        {
            return Ok(None);
        }

        // The tenant is an isolation boundary, not an implicit grant. Organization-owned
        // resources inherit the organization role because owner == tenant; team- and user-owned
        // resources are discoverable only through that owner scope or an explicit grant.
        let inherited = scope_permission(self.ctx, ownership.owner);
        let direct = if let Some(principal_id) = self.ctx.principal_id {
            self.db
                .list_effective_resource_grants(resource_type, resource_id, principal_id)
                .await
                .map_err(|_| authorization_error())?
                .into_iter()
                .map(|grant| grant.permission)
                .max()
        } else {
            None
        };
        Ok(inherited.into_iter().chain(direct).max())
    }

    /// the pipeline ids the caller can see, or `None` meaning "all" (admin / auth disabled).
    pub async fn visible_pipeline_ids(&self) -> Result<Option<HashSet<Uuid>>, Reply> {
        self.visible_resource_ids(ResourceType::Pipeline).await
    }

    /// stamp the creator as `own` on a freshly created pipeline. a no-op for service/admin
    /// principals without a user id (nothing to own it).
    pub async fn grant_pipeline_owner(&self, pipeline_id: Uuid) -> Result<(), Reply> {
        self.grant_resource_owner(ResourceType::Pipeline, pipeline_id)
            .await
    }

    /// convenience for run-scoped handlers: gate by the parent workflow's permission.
    pub async fn require_run_workflow(
        &self,
        workflow_run_id: Uuid,
        needed: Permission,
    ) -> Result<(), Reply> {
        if self.ctx.is_platform_admin() {
            return Ok(());
        }
        match self.db.fetch_workflow_run(workflow_run_id).await {
            Ok(Some(run)) => self.require_workflow(run.workflow_id, needed).await,
            _ => Err(not_found()),
        }
    }

    pub async fn require_trigger_workflow(
        &self,
        trigger_id: Uuid,
        needed: Permission,
    ) -> Result<(), Reply> {
        if self.ctx.is_platform_admin() {
            return Ok(());
        }
        match self.db.fetch_workflow_trigger(trigger_id).await {
            Ok(Some(trigger)) => self.require_workflow(trigger.workflow_id, needed).await,
            _ => Err(not_found()),
        }
    }

    /// gate a pipeline-trigger handler by its owning pipeline's permission.
    pub async fn require_pipeline_trigger(
        &self,
        trigger_id: Uuid,
        needed: Permission,
    ) -> Result<(), Reply> {
        if self.ctx.is_platform_admin() {
            return Ok(());
        }
        match self.db.fetch_pipeline_trigger(trigger_id).await {
            Ok(Some(trigger)) => self.require_pipeline(trigger.pipeline_id, needed).await,
            _ => Err(not_found()),
        }
    }

    /// gate a pipeline-run handler by its owning pipeline's permission.
    pub async fn require_pipeline_run(
        &self,
        pipeline_run_id: Uuid,
        needed: Permission,
    ) -> Result<(), Reply> {
        if self.ctx.is_platform_admin() {
            return Ok(());
        }
        match self.db.fetch_pipeline_run(pipeline_run_id).await {
            Ok(Some(run)) => self.require_pipeline(run.pipeline_id, needed).await,
            _ => Err(not_found()),
        }
    }

    pub async fn require_gate_workflow(
        &self,
        gate_id: Uuid,
        needed: Permission,
    ) -> Result<(), Reply> {
        if self.ctx.is_platform_admin() {
            return Ok(());
        }
        let workflow_run_id = match self.db.fetch_gate(gate_id).await {
            Ok(Some(record)) => record_workflow_run_id(&record),
            _ => None,
        };
        match workflow_run_id {
            Some(workflow_run_id) => self.require_run_workflow(workflow_run_id, needed).await,
            None => Err(not_found()),
        }
    }

    pub async fn require_automation_record_workflow(
        &self,
        record_type: &str,
        record_id: Uuid,
        needed: Permission,
    ) -> Result<(), Reply> {
        if self.ctx.is_platform_admin() {
            return Ok(());
        }
        let workflow_run_id = match self
            .db
            .fetch_automation_record(record_type.to_string(), record_id)
            .await
        {
            Ok(Some(record)) => record_workflow_run_id(&record),
            _ => None,
        };
        match workflow_run_id {
            Some(workflow_run_id) => self.require_run_workflow(workflow_run_id, needed).await,
            None => Err(not_found()),
        }
    }
}

pub fn record_workflow_run_id(record: &Value) -> Option<Uuid> {
    record
        .get("workflow_run_id")
        .and_then(Value::as_str)
        .and_then(|raw| raw.parse::<Uuid>().ok())
}

#[cfg(test)]
mod policy_tests {
    use super::*;
    use runinator_models::rbac::RoleAssignment;

    fn assignment(principal_id: Uuid, scope: ScopeRef, role: Role) -> RoleAssignment {
        let now = Utc::now();
        RoleAssignment {
            principal_kind: PrincipalKind::User,
            principal_id,
            scope,
            role,
            created_by: None,
            created_at: now,
            updated_at: now,
        }
    }

    fn context(
        platform_role: Option<PlatformRole>,
        assignments: Vec<RoleAssignment>,
    ) -> AuthContext {
        AuthContext {
            principal_id: assignments.first().map(|a| a.principal_id),
            session_id: None,
            kind: PrincipalKind::User,
            platform_role,
            assignments,
            system_role: None,
            action_ceiling: Vec::new(),
            org_id: None,
        }
    }

    #[test]
    fn fixed_role_action_matrix_is_deny_by_default() {
        let user = Uuid::now_v7();
        let org = ScopeRef::new(ScopeKind::Organization, Some(Uuid::now_v7())).unwrap();
        let member = context(
            None,
            vec![assignment(user, org, Role::Organization(OrgRole::Member))],
        );
        assert!(member.authorize_scope(Action::View, org));
        assert!(!member.authorize_scope(Action::Run, org));
        assert!(!member.authorize_scope(Action::MembersManage, org));

        let operator = context(
            None,
            vec![assignment(user, org, Role::Organization(OrgRole::Operator))],
        );
        assert!(operator.authorize_scope(Action::Edit, org));
        assert!(!operator.authorize_scope(Action::MembersManage, org));

        let admin = context(
            None,
            vec![assignment(user, org, Role::Organization(OrgRole::Admin))],
        );
        assert!(admin.authorize_scope(Action::MembersManage, org));
        assert!(!admin.authorize_scope(Action::Own, org));
    }

    #[test]
    fn platform_hierarchy_flows_down_and_auditor_stays_read_only() {
        let team = ScopeRef::new(ScopeKind::Team, Some(Uuid::now_v7())).unwrap();
        let auditor = context(Some(PlatformRole::Auditor), Vec::new());
        assert!(auditor.authorize_scope(Action::View, team));
        assert!(auditor.authorize_scope(Action::AuditRead, ScopeRef::PLATFORM));
        assert!(!auditor.authorize_scope(Action::Run, team));

        let operator = context(Some(PlatformRole::Operator), Vec::new());
        assert!(operator.authorize_scope(Action::Edit, team));
        assert!(!operator.authorize_scope(Action::CredentialsManage, team));

        let admin = context(Some(PlatformRole::Admin), Vec::new());
        assert!(admin.authorize_scope(Action::Own, team));
        assert!(admin.is_platform_admin());
    }

    #[test]
    fn assignments_are_additive_and_action_ceiling_restricts_keys() {
        let user = Uuid::now_v7();
        let team = ScopeRef::new(ScopeKind::Team, Some(Uuid::now_v7())).unwrap();
        let mut ctx = context(
            None,
            vec![
                assignment(user, team, Role::Team(TeamRole::Member)),
                assignment(user, team, Role::Team(TeamRole::Operator)),
            ],
        );
        assert!(ctx.authorize_scope(Action::Edit, team));
        ctx.action_ceiling = vec![Action::View];
        assert!(ctx.authorize_scope(Action::View, team));
        assert!(!ctx.authorize_scope(Action::Edit, team));
    }

    #[test]
    fn tenant_membership_does_not_leak_into_team_or_user_ownership() {
        let user = Uuid::now_v7();
        let org = ScopeRef::new(ScopeKind::Organization, Some(Uuid::now_v7())).unwrap();
        let team = ScopeRef::new(ScopeKind::Team, Some(Uuid::now_v7())).unwrap();
        let other_user = ScopeRef::new(ScopeKind::User, Some(Uuid::now_v7())).unwrap();
        let org_member = context(
            Some(PlatformRole::Member),
            vec![assignment(user, org, Role::Organization(OrgRole::Member))],
        );

        assert_eq!(scope_permission(&org_member, org), Some(Permission::View));
        assert_eq!(scope_permission(&org_member, team), None);
        assert_eq!(scope_permission(&org_member, other_user), None);

        let team_member = context(
            Some(PlatformRole::Member),
            vec![assignment(user, team, Role::Team(TeamRole::Operator))],
        );
        assert_eq!(scope_permission(&team_member, team), Some(Permission::Edit));
    }

    #[test]
    fn system_role_endpoints_honor_api_key_action_ceilings() {
        let principal_id = Uuid::now_v7();
        let mut ctx = context(None, Vec::new());
        ctx.principal_id = Some(principal_id);
        ctx.kind = PrincipalKind::Service;
        ctx.system_role = Some(SystemRole::Engine);
        assert!(ctx.require_system_role(&[SystemRole::Engine]).is_ok());

        ctx.action_ceiling = vec![Action::View];
        assert!(ctx.require_system_role(&[SystemRole::Engine]).is_err());

        ctx.action_ceiling = vec![Action::EngineOperate];
        assert!(ctx.require_system_role(&[SystemRole::Engine]).is_ok());
    }
}
