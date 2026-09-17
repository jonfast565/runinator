#[allow(unused_imports)]
use super::*;

pub(super) struct NotificationDispatcher<
    'a,
    T: RuntimeStore + NotificationStore + RunStore + WorkflowVmStore,
> {
    pub(super) db: &'a T,
    pub(super) events: &'a EventSender,
    pub(super) delivery_timeout_seconds: i64,
}

impl<T: RuntimeStore + NotificationStore + RunStore + WorkflowVmStore>
    NotificationDispatcher<'_, T>
{
    /// load the policies for one transition-based event and fire each.
    pub(super) async fn dispatch_event(
        &self,
        event: NotificationEvent,
        workflow_id: Uuid,
        context: &EmissionContext,
    ) {
        let policies = match self
            .db
            .fetch_matching_notification_policies(event, workflow_id)
            .await
        {
            Ok(policies) => policies,
            Err(err) => {
                warn!(
                    workflow_id = %workflow_id,
                    error_code = error_code_or_unknown(err.as_ref()),
                    "failed to load notification policies: {}",
                    err
                );
                return;
            }
        };
        for policy in &policies {
            self.fire(policy, context).await;
        }
    }

    /// persist the notification for a fired policy and, for external channels, enqueue its
    /// delivery.
    ///
    /// best-effort by design: a failure to alert must never fail the run that triggered it, so
    /// errors are logged and swallowed rather than propagated back into the drive path.
    pub(super) async fn fire(&self, policy: &NotificationPolicy, context: &EmissionContext) {
        let (org_id, source_resource_type, source_resource_id) =
            if let Some(run_id) = context.workflow_run_id {
                match self.db.fetch_workflow_run(run_id).await {
                    Ok(Some(run)) => (
                        repository::org_id_for_workflow_run(self.db, run_id).await,
                        Some(runinator_models::auth::ResourceType::Workflow),
                        Some(run.workflow_id),
                    ),
                    _ => (None, None, None),
                }
            } else {
                let setting_id = context
                    .metadata
                    .get("setting_id")
                    .and_then(Value::as_str)
                    .and_then(|value| value.parse().ok());
                (
                    policy.org_id,
                    setting_id.map(|_| runinator_models::auth::ResourceType::Setting),
                    setting_id,
                )
            };
        let notification = NewNotification {
            org_id,
            source_resource_type,
            source_resource_id,
            workflow_run_id: context.workflow_run_id,
            workflow_node_id: context.node_id.clone(),
            channel: policy.channel.as_str().to_string(),
            severity: policy.severity.as_str().to_string(),
            title: context.title.clone(),
            body: Some(context.body.clone()),
            target: policy.target.clone(),
            metadata: context.metadata.clone(),
            dedupe_key: Some(format!("{}:{}", policy.id, context.occurrence)),
        };
        let created = match self.db.create_notification_if_absent(&notification).await {
            Ok(Some(created)) => created,
            // already emitted for this policy/occurrence by this or another replica.
            Ok(None) => return,
            Err(err) => {
                warn!(
                    policy = %policy.id,
                    error_code = error_code_or_unknown(err.as_ref()),
                    "failed to persist notification: {}",
                    err
                );
                return;
            }
        };

        let org_id = match context.workflow_run_id {
            Some(run_id) => repository::org_id_for_workflow_run(self.db, run_id).await,
            None => None,
        };
        emit(
            self.events,
            AppEvent::new(
                org_id,
                AppEventKind::NotificationCreated {
                    notification_id: created.id,
                },
            ),
        );

        if policy.interactive
            && let Some(workflow_run_id) = context.workflow_run_id
            && let Err(error) =
                crate::services::interaction_operations::create_interaction_for_notification(
                    self.db,
                    created.id,
                    org_id,
                    workflow_run_id,
                )
                .await
        {
            warn!(notification = %created.id, %error, "failed to create notification interaction");
        }

        if policy.channel == NotificationChannel::InApp && policy.provider.is_none() {
            return;
        }
        if let Err(err) = self.enqueue_delivery(policy, &created.id, context).await {
            warn!(
                policy = %policy.id,
                notification = %created.id,
                error_code = error_code_or_unknown(err.as_ref()),
                "failed to enqueue notification delivery: {}",
                err
            );
        }
    }

    /// Freeze an external-channel delivery in its own outbox. It reuses the provider-effect
    /// executor and broker transport, but not a workflow receipt, continuation, node run, or the
    /// removed action-dispatch table.
    pub(super) async fn enqueue_delivery(
        &self,
        policy: &NotificationPolicy,
        notification_id: &Uuid,
        context: &EmissionContext,
    ) -> Result<(), SendableError> {
        let provider_action = policy
            .provider
            .as_deref()
            .zip(policy.function.as_deref())
            .or_else(|| policy.channel.provider());
        let Some((provider, function)) = provider_action else {
            return Err(crate::errors::NOTIFY_UNROUTABLE_CHANNEL.error(policy.channel.as_str()));
        };
        let Some(target) = policy.target.clone().filter(|t| !t.trim().is_empty()) else {
            return Err(crate::errors::NOTIFY_MISSING_TARGET.error(policy.id));
        };

        let delivery_id = Uuid::now_v7();
        let interaction = self
            .db
            .fetch_notification_interaction(*notification_id)
            .await?;
        let configuration = delivery_configuration(policy, &target, context, interaction.as_ref());
        let command = EffectCommand {
            version: WORKFLOW_EFFECT_PROTOCOL_VERSION,
            command_id: Uuid::now_v7(),
            // A notification can be caused by a workflow run, but is never owned by its VM
            // continuation. Global policy deliveries use nil only as a correlation placeholder.
            effect_id: delivery_id,
            workflow_run_id: context.workflow_run_id.unwrap_or_else(Uuid::nil),
            continuation_id: Uuid::nil(),
            attempt: 0,
            request: WorkflowEffectRequest::Action {
                provider: provider.to_string(),
                function: function.to_string(),
                input: configuration.into(),
                timeout_seconds: Some(self.delivery_timeout_seconds),
                retry: Default::default(),
                tags: Vec::new(),
                required_labels: Default::default(),
                workspace_affinity: None,
                execution_profile: None,
                idempotency_key: None,
                function_binding: None,
            },
            executor: EffectExecutor::Provider,
            target: Default::default(),
            trace_id: Uuid::now_v7(),
            trace_context: Default::default(),
            // The delivery id is also the immutable effect id and idempotency key. This allows a
            // redelivery to reuse worker provider protections without becoming workflow work.
            idempotency_key: format!("notification:{delivery_id}"),
            notification_delivery_id: Some(delivery_id),
        };
        self.db
            .create_notification_delivery(NewNotificationDelivery {
                id: delivery_id,
                notification_id: *notification_id,
                policy_id: Some(policy.id),
                channel: policy.channel,
                provider: Some(provider.to_string()),
                function: Some(function.to_string()),
                target: Some(target),
                workflow_run_id: context.workflow_run_id,
                command,
            })
            .await?;
        Ok(())
    }
}
