//! resource-based authorization helpers (phase 2). admins (and the synthetic admin used when auth is
//! disabled) implicitly own everything, so these short-circuit and existing behavior is unchanged
//! until grants exist.

use std::collections::HashSet;

use axum::{Json, http::StatusCode};
use chrono::Utc;
use runinator_database::interfaces::DatabaseImpl;
use runinator_models::auth::{
    AuthContext, Grant, Permission, PrincipalKind, PrincipalType, ResourceType,
};
use runinator_models::capabilities::Capability;
use runinator_models::orgs::OrgRole;
use runinator_models::revisions::{RevisionAuthor, RevisionSource};
use runinator_models::value::Value;
use uuid::Uuid;

use runinator_ws_core::models::{ApiError, ApiResponse};

type Reply = (StatusCode, Json<ApiResponse>);

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

fn workflow_kind() -> String {
    ResourceType::Workflow.as_str().to_string()
}

fn pipeline_kind() -> String {
    ResourceType::Pipeline.as_str().to_string()
}

/// pure predicates over an [`AuthContext`] with no store access. a local trait since `AuthContext`
/// lives in `runinator-models`, which stays free of ws-layer concepts (`Capability`, `Reply`).
#[allow(clippy::result_large_err)] // callers return the ready-to-send HTTP reply unchanged.
pub trait AuthContextExt {
    fn require_admin(&self) -> Result<(), Reply>;
    fn require_service_or_admin(&self) -> Result<(), Reply>;
    fn capabilities(&self) -> HashSet<Capability>;
    fn require_capability(&self, cap: Capability) -> Result<(), Reply>;
    fn require_org_role(&self, org_id: Uuid, min: OrgRole) -> Result<(), Reply>;
    fn require_org_admin(&self, org_id: Uuid) -> Result<(), Reply>;
    fn require_org_member(&self, org_id: Uuid) -> Result<(), Reply>;
    fn org_visible(&self, resource_org: Option<Uuid>) -> bool;
    fn actor_kind(&self) -> &'static str;
    fn revision_author(&self) -> RevisionAuthor;
}

impl AuthContextExt for AuthContext {
    fn require_admin(&self) -> Result<(), Reply> {
        if self.is_admin {
            Ok(())
        } else {
            Err(forbidden())
        }
    }

    fn require_service_or_admin(&self) -> Result<(), Reply> {
        if self.is_admin || matches!(self.kind, PrincipalKind::Service) {
            Ok(())
        } else {
            Err(forbidden())
        }
    }

    /// the capability set a caller holds. this is the single documented mapping of who-holds-what:
    /// platform admins (including the synthetic admin used when auth is disabled) hold every
    /// capability; admins of the caller's active org hold the org-scoped capabilities; ordinary
    /// members hold none. returned on `/auth/me` so the command center gates against the same truth
    /// the handlers enforce.
    fn capabilities(&self) -> HashSet<Capability> {
        if self.is_admin {
            return Capability::ALL.iter().copied().collect();
        }
        match self.org_role {
            Some(role) if role.allows(OrgRole::Admin) => {
                Capability::ORG_ADMIN.iter().copied().collect()
            }
            _ => HashSet::new(),
        }
    }

    /// gate an action on a named capability, else a 403 reply. platform-scoped capabilities pass
    /// only for platform admins; org-scoped capabilities pass for admins of the active org (see
    /// [`Self::capabilities`]).
    fn require_capability(&self, cap: Capability) -> Result<(), Reply> {
        if self.capabilities().contains(&cap) {
            Ok(())
        } else {
            Err(forbidden())
        }
    }

    /// gate an org-scoped action: platform admins transcend org roles; otherwise the caller's active
    /// org must match `org_id` and their role must be at least `min`.
    fn require_org_role(&self, org_id: Uuid, min: OrgRole) -> Result<(), Reply> {
        if self.is_admin {
            return Ok(());
        }
        match (self.org_id, self.org_role) {
            (Some(active), Some(role)) if active == org_id && role.allows(min) => Ok(()),
            _ => Err(forbidden()),
        }
    }

    /// require org-admin (or platform admin) for `org_id`.
    fn require_org_admin(&self, org_id: Uuid) -> Result<(), Reply> {
        self.require_org_role(org_id, OrgRole::Admin)
    }

    /// require any membership (or platform admin) in `org_id`.
    fn require_org_member(&self, org_id: Uuid) -> Result<(), Reply> {
        self.require_org_role(org_id, OrgRole::Member)
    }

    /// whether the caller may see a resource owned by `resource_org`. platform admins see
    /// everything; `None` (platform-global / unassigned) is a shared library visible to all;
    /// otherwise the caller's active org must match. this composes with, and is orthogonal to,
    /// per-resource grants.
    fn org_visible(&self, resource_org: Option<Uuid>) -> bool {
        if self.is_admin {
            return true;
        }
        match resource_org {
            None => true,
            Some(org) => self.org_id == Some(org),
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
    /// the source is inferred from the principal kind: a user token is a person working through the
    /// command center, a service key is automation. that is a proxy, not a certainty — a human with
    /// a user token and curl records as `ui`. the distinction that actually matters, pack apply
    /// versus hand edit, is stamped by the import path itself rather than inferred here.
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

/// resource-visibility checks that need both a store handle and the caller's identity. `db` and
/// `ctx` are genuinely invariant across every method here (unlike a graph cursor's node/run, which
/// varies per call), so both belong on `self` rather than threaded through each call individually.
pub struct AuthzChecker<'a, T: DatabaseImpl> {
    pub db: &'a T,
    pub ctx: &'a AuthContext,
}

impl<'a, T: DatabaseImpl> AuthzChecker<'a, T> {
    pub fn new(db: &'a T, ctx: &'a AuthContext) -> Self {
        Self { db, ctx }
    }

    /// the caller's effective permission on a workflow, or `None` when they have no access.
    pub async fn workflow_permission(&self, workflow_id: Uuid) -> Option<Permission> {
        if self.ctx.is_admin {
            return Some(Permission::Own);
        }
        let user_id = self.ctx.principal_id?;
        let team_ids = self
            .db
            .list_user_team_ids(user_id)
            .await
            .unwrap_or_default();
        let grants = self
            .db
            .list_grants(workflow_kind(), workflow_id)
            .await
            .ok()?;
        grants
            .into_iter()
            .filter(|grant| match grant.principal_type {
                PrincipalType::User => grant.principal_id == user_id,
                PrincipalType::Team => team_ids.contains(&grant.principal_id),
            })
            .map(|grant| grant.permission)
            .max()
    }

    /// require at least `needed` permission on the workflow, else a 403 reply.
    pub async fn require_workflow(
        &self,
        workflow_id: Uuid,
        needed: Permission,
    ) -> Result<(), Reply> {
        if self.ctx.is_admin {
            return Ok(());
        }
        match self.workflow_permission(workflow_id).await {
            Some(permission) if permission.allows(needed) => Ok(()),
            _ => {
                self.audit_denied(workflow_id, needed).await;
                Err(forbidden())
            }
        }
    }

    /// record an authorization denial against a workflow resource.
    async fn audit_denied(&self, workflow_id: Uuid, needed: Permission) {
        crate::audit::record_audit(
            self.db,
            self.ctx.principal_id,
            self.ctx.actor_kind(),
            "authz.denied",
            crate::audit::AuditOutcome::Denied,
            Some(ResourceType::Workflow.as_str()),
            Some(workflow_id),
            Some(&format!("missing {:?} permission", needed)),
        )
        .await;
    }

    /// the workflow ids the caller can see, or `None` meaning "all" (admin / auth disabled).
    pub async fn visible_workflow_ids(&self) -> Option<HashSet<Uuid>> {
        if self.ctx.is_admin {
            return None;
        }
        let mut ids = HashSet::new();
        // every workflow owned by the caller's active org is visible to its members, so org
        // membership grants run visibility without needing an explicit per-workflow grant. this is
        // what isolates runs by org: a caller only ever sees runs whose workflow is org-owned or
        // explicitly granted.
        if let Some(org_id) = self.ctx.org_id
            && let Ok(org_ids) = self.db.fetch_workflow_ids_for_org(org_id).await
        {
            ids.extend(org_ids);
        }
        let Some(user_id) = self.ctx.principal_id else {
            return Some(ids);
        };
        if let Ok(grants) = self.db.list_user_grants(workflow_kind(), user_id).await {
            ids.extend(grants.into_iter().map(|grant| grant.resource_id));
        }
        if let Ok(team_ids) = self.db.list_user_team_ids(user_id).await {
            for team_id in team_ids {
                if let Ok(grants) = self.db.list_team_grants(workflow_kind(), team_id).await {
                    ids.extend(grants.into_iter().map(|grant| grant.resource_id));
                }
            }
        }
        Some(ids)
    }

    /// stamp the creator as `own` on a freshly created workflow. a no-op for service/admin
    /// principals without a user id (nothing to own it).
    pub async fn grant_owner(&self, workflow_id: Uuid) {
        let Some(user_id) = self.ctx.principal_id else {
            return;
        };
        let grant = Grant {
            id: None,
            resource_type: ResourceType::Workflow,
            resource_id: workflow_id,
            principal_type: PrincipalType::User,
            principal_id: user_id,
            permission: Permission::Own,
            created_at: Utc::now(),
        };
        let _ = self.db.create_grant(grant).await;
    }

    /// the caller's effective permission on a pipeline, or `None` when they have no access.
    pub async fn pipeline_permission(&self, pipeline_id: Uuid) -> Option<Permission> {
        if self.ctx.is_admin {
            return Some(Permission::Own);
        }
        let user_id = self.ctx.principal_id?;
        let team_ids = self
            .db
            .list_user_team_ids(user_id)
            .await
            .unwrap_or_default();
        let grants = self
            .db
            .list_grants(pipeline_kind(), pipeline_id)
            .await
            .ok()?;
        grants
            .into_iter()
            .filter(|grant| match grant.principal_type {
                PrincipalType::User => grant.principal_id == user_id,
                PrincipalType::Team => team_ids.contains(&grant.principal_id),
            })
            .map(|grant| grant.permission)
            .max()
    }

    /// require at least `needed` permission on the pipeline, else a 403 reply.
    pub async fn require_pipeline(
        &self,
        pipeline_id: Uuid,
        needed: Permission,
    ) -> Result<(), Reply> {
        if self.ctx.is_admin {
            return Ok(());
        }
        match self.pipeline_permission(pipeline_id).await {
            Some(permission) if permission.allows(needed) => Ok(()),
            _ => Err(forbidden()),
        }
    }

    /// the pipeline ids the caller can see, or `None` meaning "all" (admin / auth disabled).
    pub async fn visible_pipeline_ids(&self) -> Option<HashSet<Uuid>> {
        if self.ctx.is_admin {
            return None;
        }
        let mut ids = HashSet::new();
        // every pipeline owned by the caller's active org is visible to its members.
        if let Some(org_id) = self.ctx.org_id
            && let Ok(org_ids) = self.db.fetch_pipeline_ids_for_org(org_id).await
        {
            ids.extend(org_ids);
        }
        let Some(user_id) = self.ctx.principal_id else {
            return Some(ids);
        };
        if let Ok(grants) = self.db.list_user_grants(pipeline_kind(), user_id).await {
            ids.extend(grants.into_iter().map(|grant| grant.resource_id));
        }
        if let Ok(team_ids) = self.db.list_user_team_ids(user_id).await {
            for team_id in team_ids {
                if let Ok(grants) = self.db.list_team_grants(pipeline_kind(), team_id).await {
                    ids.extend(grants.into_iter().map(|grant| grant.resource_id));
                }
            }
        }
        Some(ids)
    }

    /// stamp the creator as `own` on a freshly created pipeline. a no-op for service/admin
    /// principals without a user id (nothing to own it).
    pub async fn grant_pipeline_owner(&self, pipeline_id: Uuid) {
        let Some(user_id) = self.ctx.principal_id else {
            return;
        };
        let grant = Grant {
            id: None,
            resource_type: ResourceType::Pipeline,
            resource_id: pipeline_id,
            principal_type: PrincipalType::User,
            principal_id: user_id,
            permission: Permission::Own,
            created_at: Utc::now(),
        };
        let _ = self.db.create_grant(grant).await;
    }

    /// convenience for run-scoped handlers: gate by the parent workflow's permission.
    pub async fn require_run_workflow(
        &self,
        workflow_run_id: Uuid,
        needed: Permission,
    ) -> Result<(), Reply> {
        if self.ctx.is_admin {
            return Ok(());
        }
        match crate::repository::fetch_workflow_run(self.db, workflow_run_id).await {
            Ok(Some((run, _))) => self.require_workflow(run.workflow_id, needed).await,
            _ => Err(not_found()),
        }
    }

    pub async fn require_trigger_workflow(
        &self,
        trigger_id: Uuid,
        needed: Permission,
    ) -> Result<(), Reply> {
        if self.ctx.is_admin {
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
        if self.ctx.is_admin {
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
        if self.ctx.is_admin {
            return Ok(());
        }
        match self.db.fetch_pipeline_run(pipeline_run_id).await {
            Ok(Some(run)) => self.require_pipeline(run.pipeline_id, needed).await,
            _ => Err(not_found()),
        }
    }

    pub async fn require_node_run_workflow(
        &self,
        node_run_id: Uuid,
        needed: Permission,
    ) -> Result<(), Reply> {
        if self.ctx.is_admin {
            return Ok(());
        }
        let workflow_run_id = match self.db.fetch_workflow_node_run(node_run_id).await {
            Ok(Some(node_run)) => node_run.workflow_run_id,
            _ => return Err(not_found()),
        };
        self.require_run_workflow(workflow_run_id, needed).await
    }

    pub async fn require_gate_workflow(
        &self,
        gate_id: Uuid,
        needed: Permission,
    ) -> Result<(), Reply> {
        if self.ctx.is_admin {
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
        if self.ctx.is_admin {
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
