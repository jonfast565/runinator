//! Human conversation replies projected onto active mission control surfaces.

use std::sync::Arc;

use chrono::Utc;
use runinator_broker_core::{Broker, ControlCommand};
use runinator_models::{
    auth::{Permission, PrincipalKind, ResourceType},
    orchestration::{NormalizedAdapterEvent, OrchestrationEvidence},
    rbac::{PlatformRole, Role, RoleAssignment, ScopeKind, ScopeRef},
    runs::ProviderTerminalControl,
    value::Value,
    workflow_vm::WorkflowEffectStatus,
};
use runinator_store::{
    RuntimeStore,
    roles::{AuthStore, OrchestrationStore, RbacStore, WorkflowVmStore},
};
use uuid::Uuid;

use crate::audit::{AuditEntry, AuditOutcome, record_audit};

use super::OrchestrationOperations;

#[derive(Clone)]
pub struct ConversationControlOperations<T> {
    store: Arc<T>,
}

impl<T> ConversationControlOperations<T> {
    pub fn new(store: Arc<T>) -> Self {
        Self { store }
    }
}

impl<T> ConversationControlOperations<T>
where
    T: AuthStore + OrchestrationStore + RbacStore + RuntimeStore + WorkflowVmStore,
{
    pub async fn apply_slack_reply(
        &self,
        broker: &dyn Broker,
        organization_id: Uuid,
        event: &NormalizedAdapterEvent,
    ) -> Result<Value, String> {
        let slack_user = event
            .payload
            .get("user_id")
            .and_then(Value::as_str)
            .ok_or_else(|| "Slack reply lacks a user identity".to_string())?;
        let text = event
            .payload
            .get("text")
            .and_then(Value::as_str)
            .ok_or_else(|| "Slack reply lacks text".to_string())?;
        let command = event
            .payload
            .get("command")
            .and_then(Value::as_str)
            .unwrap_or("steer");
        let provider = event.source.clone();
        let user = self
            .store
            .fetch_user_by_identity(provider.clone(), slack_user.to_string())
            .await
            .map_err(|error| error.to_string())?;
        let Some(user) = user.filter(|user| !user.disabled) else {
            let detail = format!("unmapped external identity {provider}/{slack_user}");
            record_audit(
                self.store.as_ref(),
                AuditEntry::new(
                    None,
                    "user",
                    "slack.conversation_control",
                    AuditOutcome::Denied,
                    Some("orchestration"),
                    None,
                    Some(&detail),
                ),
            )
            .await;
            return Err("Slack user is not mapped to an enabled Runinator user".into());
        };
        let user_id = user
            .id
            .ok_or_else(|| "mapped user has no identity".to_string())?;
        let assignments = self
            .store
            .list_principal_role_assignments(PrincipalKind::User, user_id)
            .await
            .map_err(|error| error.to_string())?;
        let platform_role = assignments
            .iter()
            .filter_map(|assignment| match assignment.role {
                Role::Platform(role) => Some(role),
                _ => None,
            })
            .max();
        let alias = self
            .store
            .fetch_orchestration_correlation_alias(
                Some(organization_id),
                event.source.clone(),
                event.scope.clone(),
                event.correlation_key.clone(),
            )
            .await
            .map_err(|error| error.to_string())?
            .ok_or_else(|| "Slack thread is not correlated to a live mission".to_string())?;
        let binding = self
            .store
            .fetch_orchestration_binding(alias.binding_id)
            .await
            .map_err(|error| error.to_string())?
            .filter(|binding| binding.generation == alias.generation)
            .ok_or_else(|| "Slack thread correlation is stale".to_string())?;
        if !self
            .can_run_pipeline(user_id, platform_role, &assignments, binding.pipeline_id)
            .await?
        {
            record_audit(
                self.store.as_ref(),
                AuditEntry::new(
                    Some(user_id),
                    "user",
                    "slack.conversation_control",
                    AuditOutcome::Denied,
                    Some("orchestration"),
                    Some(binding.id),
                    Some("mapped Slack user lacks Run permission"),
                ),
            )
            .await;
            return Err("mapped Slack user is not authorized to control this mission".into());
        }

        let operations = OrchestrationOperations::new(self.store.clone());
        let action = match command {
            "approve" | "reject" => {
                let effect = operations
                    .active_mission_approval_effect(&binding)
                    .await
                    .map_err(|error| error.to_string())?
                    .ok_or_else(|| "mission has no active approval".to_string())?;
                let status = if command == "approve" {
                    WorkflowEffectStatus::Succeeded
                } else {
                    WorkflowEffectStatus::Rejected
                };
                let applied = self
                    .store
                    .settle_workflow_effect(
                        effect.id,
                        effect.attempt,
                        status,
                        Some(runinator_models::json!({
                            "decision": command,
                            "actor_id": user_id,
                            "source": provider,
                        })),
                        Some(format!("{command}d from Slack by {}", user.username)),
                        Utc::now(),
                    )
                    .await
                    .map_err(|error| error.to_string())?;
                if !applied {
                    return Err("approval was already settled or stale".into());
                }
                command.to_string()
            }
            _ => {
                let effect = operations
                    .active_mission_harness_effect(&binding)
                    .await
                    .map_err(|error| error.to_string())?
                    .ok_or_else(|| "mission has no active steerable AI phase".to_string())?;
                let replica_id = effect.current_executor_replica_id.ok_or_else(|| {
                    "mission worker has not claimed the active AI phase".to_string()
                })?;
                broker
                    .publish_control(
                        ControlCommand::for_terminal(
                            effect.workflow_run_id,
                            effect.id,
                            ProviderTerminalControl::Input {
                                data: text.to_string(),
                            },
                        )
                        .targeting_replica(replica_id),
                    )
                    .await
                    .map_err(|error| error.to_string())?;
                "steer".to_string()
            }
        };

        operations
            .append_evidence(OrchestrationEvidence {
                id: Uuid::now_v7(),
                binding_id: binding.id,
                epoch: Some(binding.current_epoch),
                kind: format!("slack_{action}"),
                subject_revision: binding.subject_revision.clone(),
                payload: runinator_models::json!({
                    "actor_id": user_id,
                    "slack_user_id": slack_user,
                    "source": event.source,
                    "channel": event.scope,
                    "thread_ts": event.correlation_key,
                    "message_ts": event.payload.get("message_ts").cloned().unwrap_or_default(),
                    "text": text,
                }),
                source_event_id: None,
                created_at: Utc::now(),
            })
            .await
            .map_err(|error| error.to_string())?;
        record_audit(
            self.store.as_ref(),
            AuditEntry::new(
                Some(user_id),
                "user",
                "slack.conversation_control",
                AuditOutcome::Success,
                Some("orchestration"),
                Some(binding.id),
                Some(action.as_str()),
            ),
        )
        .await;
        Ok(runinator_models::json!({
            "accepted": true,
            "action": action,
            "binding_id": binding.id,
            "actor_id": user_id,
        }))
    }

    async fn can_run_pipeline(
        &self,
        user_id: Uuid,
        platform_role: Option<PlatformRole>,
        assignments: &[RoleAssignment],
        pipeline_id: Uuid,
    ) -> Result<bool, String> {
        if platform_role == Some(PlatformRole::Admin) {
            return Ok(true);
        }
        let Some(ownership) = self
            .store
            .fetch_resource_ownership(ResourceType::Pipeline, pipeline_id)
            .await
            .map_err(|error| error.to_string())?
        else {
            return Ok(false);
        };
        if ownership.tenant.kind == ScopeKind::Organization
            && !scope_permission(user_id, platform_role, assignments, ownership.tenant)
                .is_some_and(|permission| permission.allows(Permission::View))
        {
            return Ok(false);
        }
        let inherited = scope_permission(user_id, platform_role, assignments, ownership.owner);
        let direct = self
            .store
            .list_effective_resource_grants(ResourceType::Pipeline, pipeline_id, user_id)
            .await
            .map_err(|error| error.to_string())?
            .into_iter()
            .map(|grant| grant.permission)
            .max();
        Ok(inherited
            .into_iter()
            .chain(direct)
            .max()
            .is_some_and(|permission| permission.allows(Permission::Run)))
    }
}

fn scope_permission(
    user_id: Uuid,
    platform_role: Option<PlatformRole>,
    assignments: &[RoleAssignment],
    scope: ScopeRef,
) -> Option<Permission> {
    if scope.kind == ScopeKind::User && scope.id == Some(user_id) {
        return Some(Permission::Own);
    }
    (scope.kind == ScopeKind::Platform)
        .then_some(platform_role)
        .flatten()
        .map(Role::Platform)
        .into_iter()
        .chain(
            assignments
                .iter()
                .filter(|assignment| assignment.scope == scope)
                .map(|assignment| assignment.role),
        )
        .map(Role::default_permission)
        .max()
}
