//! additive role permissions and API-key action ceilings.

use super::*;
use runinator_models::rbac::RoleAssignment;

#[test]
fn platform_and_local_permissions_are_additive() {
    let user = Uuid::now_v7();
    let team = ScopeRef::new(ScopeKind::Team, Some(Uuid::now_v7())).unwrap();
    let operator = context(
        Some(PlatformRole::Operator),
        vec![assignment(user, team, Role::Team(TeamRole::Owner))],
    );
    assert!(operator.authorize_scope(Action::AuditRead, team));
    assert!(operator.authorize_scope(Action::CredentialsManage, team));

    let auditor = context(
        Some(PlatformRole::Auditor),
        vec![assignment(user, team, Role::Team(TeamRole::Operator))],
    );
    assert!(auditor.authorize_scope(Action::AuditRead, team));
    assert!(auditor.authorize_scope(Action::Edit, team));
    assert!(!auditor.authorize_scope(Action::CredentialsManage, team));
}

#[test]
fn system_role_ceiling_must_allow_the_principals_own_role() {
    let mut ctx = context(None, Vec::new());
    ctx.kind = PrincipalKind::Service;
    ctx.system_role = Some(SystemRole::Worker);
    ctx.action_ceiling = vec![Action::AgentOperate];
    let roles = [SystemRole::Worker, SystemRole::Agent];
    assert!(ctx.require_system_role(&roles).is_err());
    ctx.action_ceiling = vec![Action::WorkerOperate];
    assert!(ctx.require_system_role(&roles).is_ok());
    ctx.system_role = None;
    assert!(ctx.require_system_role(&roles).is_err());
    ctx.platform_role = Some(PlatformRole::Admin);
    assert!(ctx.require_system_role(&roles).is_ok());
    ctx.action_ceiling = vec![Action::View];
    assert!(ctx.require_system_role(&roles).is_err());
}

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

fn context(platform_role: Option<PlatformRole>, assignments: Vec<RoleAssignment>) -> AuthContext {
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
