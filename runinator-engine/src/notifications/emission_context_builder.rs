#[allow(unused_imports)]
use super::*;

pub(super) struct EmissionContextBuilder<
    'a,
    T: RuntimeStore + NotificationStore + RunStore + WorkflowVmStore,
> {
    pub(super) db: &'a T,
    pub(super) run: &'a WorkflowRun,
}

impl<T: RuntimeStore + NotificationStore + RunStore + WorkflowVmStore>
    EmissionContextBuilder<'_, T>
{
    /// render the message for a failed run.
    pub(super) async fn run_failed(&self) -> EmissionContext {
        let run = self.run;
        let workflow_name = self.workflow_name().await;
        let reason = run
            .message
            .clone()
            .unwrap_or_else(|| "no failure message recorded".to_string());
        EmissionContext {
            workflow_run_id: Some(run.id),
            node_id: run.active_node_id.clone(),
            title: format!("{} {}", workflow_name, run.status.as_str()),
            body: format!(
                "Run {} ended {}{}.\n{}",
                run.id,
                run.status.as_str(),
                run.active_node_id
                    .as_ref()
                    .map(|node| format!(" at node '{node}'"))
                    .unwrap_or_default(),
                reason
            ),
            metadata: runinator_models::json!({
                "event": NotificationEvent::RunFailed.as_str(),
                "workflow_id": run.workflow_id,
                "workflow_run_id": run.id,
                "status": run.status.as_str(),
            }),
            // one occurrence per run terminal: a run settles once, so the run id is the whole
            // identity.
            occurrence: format!("run_failed:{}", run.id),
        }
    }

    /// render the message for the node in a failed run that used up its retry budget, if there is
    /// one.
    pub(super) async fn retry_exhausted(&self) -> Option<EmissionContext> {
        let run = self.run;
        let module = self.db.fetch_workflow_module(run.id).await.ok()??;
        let exhausted = self
            .db
            .fetch_workflow_effects(run.id)
            .await
            .ok()?
            .into_iter()
            // An action retry is an effect retry. The receipt's request, attempt, and frozen
            // continuation are the durable replacement for a node-run attempt row.
            .filter(|effect| {
                effect.status == runinator_models::workflow_vm::WorkflowEffectStatus::Failed
                    && effect.attempt > 0
                    && matches!(
                        effect.request,
                        runinator_models::workflow_vm::WorkflowEffectRequest::Action { .. }
                    )
            })
            .max_by_key(|effect| effect.attempt)?;
        let continuation = self
            .db
            .fetch_workflow_continuation(exhausted.continuation_id)
            .await
            .ok()??;
        let node_id = module
            .graph_location(continuation.instruction_pointer.saturating_sub(1))?
            .node_id
            .clone();
        let workflow_name = self.workflow_name().await;
        Some(EmissionContext {
            workflow_run_id: Some(run.id),
            node_id: Some(node_id.clone()),
            title: format!("{} exhausted retries on '{}'", workflow_name, node_id),
            body: format!(
                "Node '{}' in run {} failed after {} attempt(s).\n{}",
                node_id,
                run.id,
                exhausted.attempt + 1,
                exhausted
                    .message
                    .clone()
                    .unwrap_or_else(|| "no failure message recorded".to_string())
            ),
            metadata: runinator_models::json!({
                "event": NotificationEvent::NodeRetryExhausted.as_str(),
                "workflow_id": run.workflow_id,
                "workflow_run_id": run.id,
                "node_id": node_id,
                "attempts": exhausted.attempt + 1,
            }),
            occurrence: format!("retry_exhausted:{}:{node_id}", run.id),
        })
    }

    /// render the message for a duration breach.
    pub(super) async fn duration(
        &self,
        event: NotificationEvent,
        threshold_seconds: i64,
        age_seconds: i64,
    ) -> EmissionContext {
        let run = self.run;
        let workflow_name = self.workflow_name().await;
        let (title, verb) = match event {
            NotificationEvent::RunParked => (
                format!("{workflow_name} parked over threshold"),
                "has been parked",
            ),
            _ => (format!("{workflow_name} breached SLA"), "has been open"),
        };
        EmissionContext {
            workflow_run_id: Some(run.id),
            node_id: run.active_node_id.clone(),
            title,
            body: format!(
                "Run {} {} for {} (threshold {}), currently {}{}.",
                run.id,
                verb,
                humanize_seconds(age_seconds),
                humanize_seconds(threshold_seconds),
                run.status.as_str(),
                run.active_node_id
                    .as_ref()
                    .map(|node| format!(" at node '{node}'"))
                    .unwrap_or_default(),
            ),
            metadata: runinator_models::json!({
                "event": event.as_str(),
                "workflow_id": run.workflow_id,
                "workflow_run_id": run.id,
                "status": run.status.as_str(),
                "threshold_seconds": threshold_seconds,
                "age_seconds": age_seconds,
            }),
            // bucket by threshold so a policy alerts once per run, but re-alerts if an operator
            // raises the threshold and the run breaches the new one too.
            occurrence: format!("{}:{}:{}", event.as_str(), run.id, threshold_seconds),
        }
    }

    /// prefer the run's own snapshot for the workflow name so an alert names the definition that
    /// actually ran, falling back to the live row and finally the id.
    pub(super) async fn workflow_name(&self) -> String {
        let run = self.run;
        if let Some(snapshot) = run.workflow_snapshot.as_ref()
            && !snapshot.name.trim().is_empty()
        {
            return snapshot.name.clone();
        }
        if let Ok(Some(workflow)) = self.db.fetch_workflow(run.workflow_id).await {
            return workflow.name;
        }
        run.workflow_id.to_string()
    }
}
