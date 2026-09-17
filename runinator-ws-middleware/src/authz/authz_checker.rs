#[allow(unused_imports)]
use super::*;

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

    /// Whether the caller may act on a scope, including organization inheritance for team scopes.
    pub async fn authorize_scope_with_ancestry(
        &self,
        action: Action,
        scope: ScopeRef,
    ) -> Result<bool, Reply> {
        if self.ctx.authorize_scope(action, scope) {
            return Ok(true);
        }
        if scope.kind != ScopeKind::Team {
            return Ok(false);
        }
        let Some(team_id) = scope.id else {
            return Ok(false);
        };
        let Some(team) = self
            .db
            .fetch_team(team_id)
            .await
            .map_err(|_| authorization_error())?
        else {
            return Ok(false);
        };
        Ok(self.ctx.authorize_scope(action, team.scope))
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
    pub(super) async fn audit_resource_denied(
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
        if !ceiling_allows(self.ctx, Permission::View) {
            return Ok(Some(HashSet::new()));
        }
        if self.ctx.is_platform_admin() {
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

    /// require at least `needed` permission on an orchestration adapter.
    pub async fn require_adapter(&self, adapter_id: Uuid, needed: Permission) -> Result<(), Reply> {
        self.require_resource(ResourceType::OrchestrationAdapter, adapter_id, needed)
            .await
    }

    pub async fn resource_permission(
        &self,
        resource_type: ResourceType,
        resource_id: Uuid,
    ) -> Result<Option<Permission>, Reply> {
        runinator_store::resource_access::effective_resource_permission(
            self.db,
            self.ctx,
            resource_type,
            resource_id,
        )
        .await
        .map_err(|_| authorization_error())
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
        if !ceiling_allows(self.ctx, needed) {
            return Err(forbidden());
        }
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
        if !ceiling_allows(self.ctx, needed) {
            return Err(forbidden());
        }
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
        if !ceiling_allows(self.ctx, needed) {
            return Err(forbidden());
        }
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
        if !ceiling_allows(self.ctx, needed) {
            return Err(forbidden());
        }
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
        if !ceiling_allows(self.ctx, needed) {
            return Err(forbidden());
        }
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
        if !ceiling_allows(self.ctx, needed) {
            return Err(forbidden());
        }
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
