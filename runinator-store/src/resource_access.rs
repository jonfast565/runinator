//! Ownership-aware reusable-resource dependency checks shared by authoring and runtime admission.

use runinator_models::{
    auth::{AuthContext, Permission, PrincipalType, ResourceType},
    errors::SendableError,
    rbac::{Role, ScopeKind, ScopeRef},
};
use uuid::Uuid;

use crate::roles::{AuthStore, RbacStore};

/// Resolve the strongest permission a request context holds on an owned resource.
///
/// This is the canonical store-backed RBAC calculation used by both HTTP authorization and
/// engine-owned interaction handling. Transport layers remain responsible for mapping a missing
/// permission to their own denial response and for enforcing action ceilings.
pub async fn effective_resource_permission<T: RbacStore>(
    db: &T,
    ctx: &AuthContext,
    resource_type: ResourceType,
    resource_id: Uuid,
) -> Result<Option<Permission>, SendableError> {
    if ctx.is_platform_admin() {
        return Ok(Some(Permission::Own));
    }
    let Some(ownership) = db
        .fetch_resource_ownership(resource_type, resource_id)
        .await?
    else {
        return Ok(None);
    };
    if ownership.tenant.kind == ScopeKind::Organization
        && (!(ctx.action_ceiling.is_empty()
            || ctx
                .action_ceiling
                .contains(&runinator_models::rbac::Action::View))
            || !scope_permission(ctx, ownership.tenant)
                .is_some_and(|permission| permission.allows(Permission::View)))
    {
        return Ok(None);
    }

    let inherited = scope_permission(ctx, ownership.owner);
    let direct = if let Some(principal_id) = ctx.principal_id {
        db.list_effective_resource_grants(resource_type, resource_id, principal_id)
            .await?
            .into_iter()
            .map(|grant| grant.permission)
            .max()
    } else {
        None
    };
    Ok(inherited.into_iter().chain(direct).max())
}

/// Resolve the permission inherited from a platform, organization, team, or user scope.
pub fn scope_permission(ctx: &AuthContext, scope: ScopeRef) -> Option<Permission> {
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

/// Return whether an owned consumer may use an owned reusable dependency.
pub async fn resource_can_consume<T: AuthStore + RbacStore>(
    db: &T,
    consumer_type: ResourceType,
    consumer_id: Uuid,
    dependency_type: ResourceType,
    dependency_id: Uuid,
) -> Result<bool, SendableError> {
    let Some(consumer) = db
        .fetch_resource_ownership(consumer_type, consumer_id)
        .await?
    else {
        return Ok(false);
    };
    owner_can_consume(
        db,
        consumer.owner,
        consumer.tenant,
        dependency_type,
        dependency_id,
    )
    .await
}

/// Check a prospective owner/tenant pair before the consumer itself has been persisted.
pub async fn owner_can_consume<T: AuthStore + RbacStore>(
    db: &T,
    consumer_owner: ScopeRef,
    consumer_tenant: ScopeRef,
    dependency_type: ResourceType,
    dependency_id: Uuid,
) -> Result<bool, SendableError> {
    owner_can_access(
        db,
        consumer_owner,
        consumer_tenant,
        dependency_type,
        dependency_id,
        Permission::Run,
    )
    .await
}

pub async fn owner_can_access<T: AuthStore + RbacStore>(
    db: &T,
    consumer_owner: ScopeRef,
    consumer_tenant: ScopeRef,
    dependency_type: ResourceType,
    dependency_id: Uuid,
    needed: Permission,
) -> Result<bool, SendableError> {
    let Some(dependency) = db
        .fetch_resource_ownership(dependency_type, dependency_id)
        .await?
    else {
        return Ok(false);
    };
    if consumer_tenant != dependency.tenant {
        return Ok(false);
    }
    if dependency.owner == dependency.tenant || consumer_owner == dependency.owner {
        return Ok(true);
    }
    if consumer_owner.kind == ScopeKind::User
        && dependency.owner.kind == ScopeKind::Team
        && let (Some(user_id), Some(team_id)) = (consumer_owner.id, dependency.owner.id)
        && db.list_user_team_ids(user_id).await?.contains(&team_id)
    {
        return Ok(true);
    }
    let grants = db
        .list_grants(dependency_type.as_str().to_string(), dependency_id)
        .await?;
    for grant in grants
        .into_iter()
        .filter(|grant| grant.permission.allows(needed))
    {
        match (consumer_owner.kind, consumer_owner.id, grant.principal_type) {
            (ScopeKind::User, Some(user_id), PrincipalType::User)
                if grant.principal_id == user_id =>
            {
                return Ok(true);
            }
            (ScopeKind::Team, Some(team_id), PrincipalType::Team)
                if grant.principal_id == team_id =>
            {
                return Ok(true);
            }
            (ScopeKind::User, Some(user_id), PrincipalType::Team)
                if db
                    .list_user_team_ids(user_id)
                    .await?
                    .contains(&grant.principal_id) =>
            {
                return Ok(true);
            }
            _ => {}
        }
    }
    Ok(false)
}
