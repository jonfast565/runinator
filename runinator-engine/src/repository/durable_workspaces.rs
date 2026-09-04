//! Portable workspace admission and management.
use chrono::Utc;
use runinator_models::{
    auth::{Permission, ResourceType},
    errors::{SendableError, WORKSPACE_CONFLICT, WORKSPACE_INVALID},
    workflow_vm::WorkflowEffectRequest,
    workspaces::*,
};
use runinator_store::{
    RuntimeStore,
    roles::{AuthStore, DurableWorkspaceStore, RbacStore},
};

pub async fn prepare_dispatch<T: RuntimeStore + AuthStore + RbacStore + DurableWorkspaceStore>(
    db: &T,
    command: &mut runinator_comm::EffectCommand,
) -> Result<bool, SendableError> {
    let WorkflowEffectRequest::Action {
        workspace_affinity: Some(value),
        timeout_seconds,
        ..
    } = &mut command.request
    else {
        return Ok(true);
    };
    if value.get("key").is_none() {
        return Ok(true);
    }
    let mut attachment: WorkspaceAttachment = serde_json::from_value(value.clone().into())
        .map_err(|error| WORKSPACE_INVALID.error(error))?;
    validate_key(&attachment.reference.key)?;
    let run = db
        .fetch_workflow_run(command.workflow_run_id)
        .await?
        .ok_or_else(|| WORKSPACE_INVALID.error("consuming run is missing"))?;
    let owner = db
        .fetch_resource_ownership(ResourceType::Workflow, run.workflow_id)
        .await?
        .ok_or_else(|| WORKSPACE_INVALID.error("consuming workflow has no owner"))?;
    let org_id = owner.tenant.id;
    let mut workspace = db
        .resolve_durable_workspace(org_id, attachment.reference.key.clone())
        .await?;
    if workspace.is_none() && attachment.create {
        let now = Utc::now();
        let identity = DurableWorkspace {
            id: uuid::Uuid::now_v7(),
            key: attachment.reference.key.clone(),
            org_id,
            head_version: 0,
            revision: 1,
            deleted_at: None,
            created_at: now,
            updated_at: now,
        };
        let mut ownership = owner.clone();
        ownership.resource_type = ResourceType::Workspace;
        ownership.resource_id = identity.id;
        ownership.created_at = now;
        ownership.updated_at = now;
        workspace = Some(db.create_durable_workspace(identity, ownership).await?);
    }
    let workspace =
        workspace.ok_or_else(|| WORKSPACE_INVALID.error("workspace key does not exist"))?;
    let permission = if attachment.access == WorkspaceAccess::Write {
        Permission::Edit
    } else {
        Permission::View
    };
    if !runinator_store::resource_access::owner_can_access(
        db,
        owner.owner,
        owner.tenant,
        ResourceType::Workspace,
        workspace.id,
        permission,
    )
    .await?
    {
        return Err(WORKSPACE_INVALID.error("workflow owner cannot access this workspace"));
    }
    if attachment.follow_run {
        attachment.reference.version = db
            .workspace_version_for_run(workspace.id, command.workflow_run_id)
            .await?
            .or(attachment.reference.version);
    }
    let now = Utc::now();
    let timeout = timeout_seconds
        .unwrap_or(runinator_models::workflow_vm::DEFAULT_ACTION_TIMEOUT_SECONDS)
        .max(1);
    let acquisition = db
        .acquire_workspace_checkout(WorkspaceAcquire {
            workspace_id: workspace.id,
            workflow_run_id: command.workflow_run_id,
            effect_id: command.effect_id,
            attempt: command.attempt,
            version: attachment.reference.version,
            access: attachment.access,
            now,
            leased_until: now + chrono::Duration::seconds(timeout.saturating_add(300)),
        })
        .await?;
    let checkout = match acquisition {
        WorkspaceAcquisition::Acquired { checkout } => checkout,
        WorkspaceAcquisition::Busy => return Ok(false),
        WorkspaceAcquisition::Conflict => {
            return Err(WORKSPACE_CONFLICT.error("pinned version is no longer the head"));
        }
        WorkspaceAcquisition::Missing => {
            return Err(WORKSPACE_INVALID.error("workspace or pinned version was deleted"));
        }
    };
    let snapshot = if checkout.base_version == 0 {
        None
    } else {
        Some(
            db.fetch_workspace_snapshot(workspace.id, checkout.base_version)
                .await?
                .ok_or_else(|| WORKSPACE_INVALID.error("pinned version was deleted"))?,
        )
    };
    *value = runinator_models::value::Value::encode(&WorkspaceExecution {
        key: workspace.key,
        checkout,
        snapshot,
        results: attachment.results,
    })?;
    Ok(true)
}

pub fn validate_key(key: &str) -> Result<(), SendableError> {
    if key.is_empty() || key.len() > 200 || key.trim() != key || key.chars().any(char::is_control) {
        return Err(WORKSPACE_INVALID.error(
            "key must contain 1–200 bytes without surrounding whitespace or control characters",
        ));
    }
    Ok(())
}
