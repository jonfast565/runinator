//! Provider-neutral actions projected from notifications onto durable workflow controls.

use std::sync::Arc;

use chrono::Utc;
use runinator_broker_core::{Broker, ControlCommand};
use runinator_models::{
    auth::{AuthContext, Permission, PrincipalKind, ResourceType},
    notifications::{
        ConversationReceipt, ExternalInteractionResponse, NotificationInteraction,
        NotificationInteractionAction, NotificationInteractionInput, NotificationInteractionState,
        NotificationInteractionTarget,
    },
    rbac::strongest_platform_role,
    runs::ProviderTerminalControl,
    value::Value,
    workflow_vm::{WorkflowEffectRequest, WorkflowEffectStatus},
};
use runinator_store::{
    RuntimeStore,
    roles::{AuthStore, NewNotificationInteraction, NotificationStore, RbacStore, WorkflowVmStore},
};
use uuid::Uuid;

use crate::audit::{AuditEntry, AuditOutcome, record_audit};

fn rejected(detail: impl std::fmt::Display) -> String {
    crate::errors::INTERACTION_REJECTED
        .error(detail)
        .to_string()
}

#[derive(Clone)]
pub struct InteractionOperations<T> {
    store: Arc<T>,
}

impl<T> InteractionOperations<T> {
    pub fn new(store: Arc<T>) -> Self {
        Self { store }
    }
}

impl<T> InteractionOperations<T>
where
    T: NotificationStore + RuntimeStore + WorkflowVmStore,
{
    pub async fn create_for_notification(
        &self,
        notification_id: Uuid,
        org_id: Option<Uuid>,
        workflow_run_id: Uuid,
    ) -> Result<Option<NotificationInteraction>, String> {
        create_interaction_for_notification(
            self.store.as_ref(),
            notification_id,
            org_id,
            workflow_run_id,
        )
        .await
    }

    pub async fn hydrate(
        &self,
        mut notification: runinator_models::notifications::Notification,
    ) -> Result<runinator_models::notifications::Notification, String> {
        notification.interaction = self
            .store
            .fetch_notification_interaction(notification.id)
            .await
            .map_err(|error| error.to_string())?;
        Ok(notification)
    }

    pub async fn apply(
        &self,
        broker: &dyn Broker,
        interaction: NotificationInteraction,
        actor_id: Uuid,
        action_id: &str,
        input: Value,
    ) -> Result<Value, String> {
        if interaction.state != NotificationInteractionState::Open {
            return Err(rejected("notification interaction is no longer open"));
        }
        let action = interaction
            .actions
            .iter()
            .find(|action| action.id == action_id)
            .ok_or_else(|| rejected("action is not allowed for this notification"))?;
        validate_action_input(action, &input)?;

        let effect = self
            .store
            .fetch_workflow_effect(interaction.target.effect_id())
            .await
            .map_err(|error| error.to_string())?
            .filter(|effect| {
                effect.workflow_run_id == interaction.target.workflow_run_id()
                    && effect.attempt == interaction.target.attempt()
                    && !effect.status.is_terminal()
            });
        let Some(effect) = effect else {
            let _ = self
                .store
                .mark_notification_interaction_stale(interaction.id, Utc::now())
                .await;
            return Err(rejected(
                "notification interaction targets a stale effect attempt",
            ));
        };

        let resolves = match (&interaction.target, action_id) {
            (NotificationInteractionTarget::Effect { .. }, "approve") => {
                self.settle(
                    &effect,
                    WorkflowEffectStatus::Succeeded,
                    runinator_models::json!({ "decision": "approved", "actor_id": actor_id }),
                    "approval approved",
                )
                .await?;
                true
            }
            (NotificationInteractionTarget::Effect { .. }, "reject") => {
                self.settle(
                    &effect,
                    WorkflowEffectStatus::Rejected,
                    runinator_models::json!({ "decision": "rejected", "actor_id": actor_id }),
                    "approval rejected",
                )
                .await?;
                true
            }
            (NotificationInteractionTarget::Effect { .. }, "open") => {
                self.settle(
                    &effect,
                    WorkflowEffectStatus::Succeeded,
                    runinator_models::json!({ "open": true, "actor_id": actor_id }),
                    "gate opened",
                )
                .await?;
                true
            }
            (NotificationInteractionTarget::Effect { .. }, "close") => {
                self.settle(
                    &effect,
                    WorkflowEffectStatus::Succeeded,
                    runinator_models::json!({ "open": false, "actor_id": actor_id }),
                    "gate closed",
                )
                .await?;
                true
            }
            (NotificationInteractionTarget::Effect { .. }, "submit") => {
                self.settle(
                    &effect,
                    WorkflowEffectStatus::Succeeded,
                    runinator_models::json!({ "input": input, "actor_id": actor_id }),
                    "input submitted",
                )
                .await?;
                true
            }
            (NotificationInteractionTarget::Signal { name, .. }, "signal") => {
                self.settle(
                    &effect,
                    WorkflowEffectStatus::Succeeded,
                    runinator_models::json!({ "signal": name, "payload": input }),
                    "signal received",
                )
                .await?;
                true
            }
            (NotificationInteractionTarget::Terminal { .. }, "steer") => {
                let replica_id = effect
                    .current_executor_replica_id
                    .ok_or_else(|| rejected("terminal worker has not claimed the active effect"))?;
                let text = input
                    .as_str()
                    .filter(|text| !text.trim().is_empty())
                    .ok_or_else(|| rejected("terminal input must be non-empty text"))?;
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
                false
            }
            _ => return Err(rejected("action does not match the notification target")),
        };

        if resolves
            && !self
                .store
                .resolve_notification_interaction(
                    interaction.id,
                    action_id.to_string(),
                    actor_id,
                    Utc::now(),
                )
                .await
                .map_err(|error| error.to_string())?
        {
            return Err(rejected("notification interaction was already resolved"));
        }
        record_audit(
            self.store.as_ref(),
            AuditEntry::new(
                Some(actor_id),
                "user",
                "notification.interaction",
                AuditOutcome::Success,
                Some("workflow_run"),
                Some(effect.workflow_run_id),
                Some(action_id),
            ),
        )
        .await;
        Ok(runinator_models::json!({
            "accepted": true,
            "action": action_id,
            "interaction_id": interaction.id,
            "resolved": resolves,
        }))
    }

    async fn settle(
        &self,
        effect: &runinator_models::workflow_vm::WorkflowEffect,
        status: WorkflowEffectStatus,
        output: Value,
        message: &str,
    ) -> Result<(), String> {
        let applied = self
            .store
            .settle_workflow_effect(
                effect.id,
                effect.attempt,
                status,
                Some(output),
                Some(message.into()),
                Utc::now(),
            )
            .await
            .map_err(|error| error.to_string())?;
        if applied {
            Ok(())
        } else {
            Err(rejected("workflow effect was already settled or stale"))
        }
    }
}

pub(crate) async fn create_interaction_for_notification<T: NotificationStore + WorkflowVmStore>(
    store: &T,
    notification_id: Uuid,
    org_id: Option<Uuid>,
    workflow_run_id: Uuid,
) -> Result<Option<NotificationInteraction>, String> {
    let effects = store
        .fetch_workflow_effects(workflow_run_id)
        .await
        .map_err(|error| error.to_string())?;
    let Some((target, actions)) = effects
        .into_iter()
        .rev()
        .filter(|effect| !effect.status.is_terminal())
        .find_map(|effect| interaction_contract(&effect))
    else {
        return Ok(None);
    };
    store
        .create_notification_interaction(NewNotificationInteraction {
            id: Uuid::now_v7(),
            notification_id,
            org_id,
            target,
            actions,
        })
        .await
        .map(Some)
        .map_err(|error| error.to_string())
}

impl<T> InteractionOperations<T>
where
    T: AuthStore + NotificationStore + RbacStore + RuntimeStore + WorkflowVmStore,
{
    pub async fn apply_external(
        &self,
        broker: &dyn Broker,
        org_id: Option<Uuid>,
        receipt: ConversationReceipt,
        response: ExternalInteractionResponse,
    ) -> Result<Value, String> {
        let interaction = self
            .store
            .fetch_notification_interaction_by_conversation(org_id, receipt.clone())
            .await
            .map_err(|error| error.to_string())?
            .ok_or_else(|| rejected("conversation is not bound to an actionable notification"))?;
        let user = self
            .store
            .fetch_user_by_identity(receipt.source.clone(), response.actor_subject.clone())
            .await
            .map_err(|error| error.to_string())?;
        let Some(user) = user.filter(|user| !user.disabled) else {
            record_audit(
                self.store.as_ref(),
                AuditEntry::new(
                    None,
                    "user",
                    "notification.interaction",
                    AuditOutcome::Denied,
                    Some("workflow_run"),
                    Some(interaction.target.workflow_run_id()),
                    Some("external identity is not mapped to an enabled user"),
                ),
            )
            .await;
            return Err(rejected(
                "external identity is not mapped to an enabled Runinator user",
            ));
        };
        let user_id = user
            .id
            .ok_or_else(|| rejected("mapped user has no identity"))?;
        if !self
            .can_run_workflow(user_id, interaction.target.workflow_run_id())
            .await?
        {
            record_audit(
                self.store.as_ref(),
                AuditEntry::new(
                    Some(user_id),
                    "user",
                    "notification.interaction",
                    AuditOutcome::Denied,
                    Some("workflow_run"),
                    Some(interaction.target.workflow_run_id()),
                    Some("mapped external user lacks Run permission"),
                ),
            )
            .await;
            return Err(rejected(
                "mapped user is not authorized to control this workflow run",
            ));
        }
        self.apply(
            broker,
            interaction,
            user_id,
            &response.action_id,
            response.input,
        )
        .await
    }

    async fn can_run_workflow(&self, user_id: Uuid, run_id: Uuid) -> Result<bool, String> {
        let run = self
            .store
            .fetch_workflow_run(run_id)
            .await
            .map_err(|error| error.to_string())?
            .ok_or_else(|| rejected("workflow run not found"))?;
        let assignments = self
            .store
            .list_principal_role_assignments(PrincipalKind::User, user_id)
            .await
            .map_err(|error| error.to_string())?;
        let platform_role = strongest_platform_role(&assignments);
        let ctx = AuthContext {
            principal_id: Some(user_id),
            session_id: None,
            kind: PrincipalKind::User,
            platform_role,
            assignments,
            system_role: None,
            action_ceiling: Vec::new(),
            org_id: None,
        };
        Ok(
            runinator_store::resource_access::effective_resource_permission(
                self.store.as_ref(),
                &ctx,
                ResourceType::Workflow,
                run.workflow_id,
            )
            .await
            .map_err(|error| error.to_string())?
            .is_some_and(|permission| permission.allows(Permission::Run)),
        )
    }
}

pub(crate) fn interaction_contract(
    effect: &runinator_models::workflow_vm::WorkflowEffect,
) -> Option<(
    NotificationInteractionTarget,
    Vec<NotificationInteractionAction>,
)> {
    let effect_target = || NotificationInteractionTarget::Effect {
        workflow_run_id: effect.workflow_run_id,
        effect_id: effect.id,
        attempt: effect.attempt,
    };
    let action = |id: &str, label: &str, input| NotificationInteractionAction {
        id: id.into(),
        label: label.into(),
        input,
    };
    match &effect.request {
        WorkflowEffectRequest::Approval { .. } => Some((
            effect_target(),
            vec![
                action("approve", "Approve", NotificationInteractionInput::None),
                action("reject", "Reject", NotificationInteractionInput::None),
            ],
        )),
        WorkflowEffectRequest::Gate { .. } => Some((
            effect_target(),
            vec![
                action("open", "Open", NotificationInteractionInput::None),
                action("close", "Close", NotificationInteractionInput::None),
            ],
        )),
        WorkflowEffectRequest::Signal { key, .. } => Some((
            NotificationInteractionTarget::Signal {
                workflow_run_id: effect.workflow_run_id,
                effect_id: effect.id,
                attempt: effect.attempt,
                name: key.clone(),
            },
            vec![action(
                "signal",
                "Send signal",
                NotificationInteractionInput::Json,
            )],
        )),
        WorkflowEffectRequest::Input { .. } => Some((
            effect_target(),
            vec![action(
                "submit",
                "Submit",
                NotificationInteractionInput::Json,
            )],
        )),
        WorkflowEffectRequest::Action {
            provider,
            function,
            input,
            ..
        } if ((provider == "console" && function == "run")
            || (provider == "ai-command" && function == "claude_code"))
            && (input.get("interactive").and_then(Value::as_bool) == Some(true)
                || input.get("harnessed").and_then(Value::as_bool) == Some(true)) =>
        {
            Some((
                NotificationInteractionTarget::Terminal {
                    workflow_run_id: effect.workflow_run_id,
                    effect_id: effect.id,
                    attempt: effect.attempt,
                },
                vec![action(
                    "steer",
                    "Send input",
                    NotificationInteractionInput::Text,
                )],
            ))
        }
        _ => None,
    }
}

fn validate_action_input(
    action: &NotificationInteractionAction,
    input: &Value,
) -> Result<(), String> {
    match action.input {
        NotificationInteractionInput::None if !input.is_null() => {
            Err(rejected("this action does not accept input"))
        }
        NotificationInteractionInput::Text
            if input.as_str().is_none_or(|value| value.trim().is_empty()) =>
        {
            Err(rejected("this action requires non-empty text input"))
        }
        _ => Ok(()),
    }
}
