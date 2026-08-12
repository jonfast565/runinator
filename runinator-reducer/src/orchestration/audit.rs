use super::context::runtime_context;
use super::transitions::transition_from_node;
use super::*;

/// build the audit record payload from resolved node parameters. exposed for unit testing.
pub(super) fn build_audit_record(workflow_run_id: Uuid, node_id: &str, resolved: &Value) -> Value {
    runinator_models::json!({
        "workflow_run_id": workflow_run_id,
        "node_id": node_id,
        "actor": resolved.get("actor").cloned().unwrap_or(Value::Null),
        "action": resolved.get("action").and_then(Value::as_str).unwrap_or("unknown"),
        "target": resolved.get("target").cloned().unwrap_or(Value::Null),
        "reason": resolved.get("reason").cloned().unwrap_or(Value::Null),
        "metadata": resolved.get("metadata").cloned().unwrap_or(Value::Null),
    })
}

/// process an audit node: resolves parameters, inserts an immutable audit-log row, and emits a
/// structured output. the existing `record_audit_log` database method is used so no new schema is
/// required.
pub(super) struct AuditHandler;

impl<T: ReducerStore> super::handler::NodeHandler<T> for AuditHandler {
    async fn process<'a>(
        &'a self,
        ctx: &'a super::handler::NodeHandlerContext<'a, T>,
    ) -> Result<ReadyNodeDisposition, SendableError>
    where
        T: 'a,
    {
        let node_run = ctx
            .db
            .create_workflow_node_run(
                ctx.workflow_run.id,
                ctx.node.id.clone(),
                ctx.node.parameters.clone().into(),
                super::context::most_recently_finished_node_run(ctx.node_runs),
                Some(ctx.cursor),
            )
            .await?;
        let context = runtime_context(ctx).await;
        let params: Value = ctx.node.parameters.clone().into();
        let resolved = runinator_workflows::resolve_value_refs(&params, &context)
            .map_err(|err| -> SendableError { Box::new(err) })?;
        let record = build_audit_record(ctx.workflow_run.id, &ctx.node.id, &resolved);
        let inserted = ctx.db.record_audit_log(record).await?;
        let audit_id = inserted
            .get("id")
            .and_then(Value::as_str)
            .and_then(|s| s.parse::<Uuid>().ok());
        let output = AuditOutput {
            id: audit_id,
            actor: resolved
                .get("actor")
                .and_then(Value::as_str)
                .map(str::to_string),
            action: resolved
                .get("action")
                .and_then(Value::as_str)
                .unwrap_or("unknown")
                .to_string(),
            target: resolved
                .get("target")
                .and_then(Value::as_str)
                .map(str::to_string),
        };
        transition_from_node(
            ctx,
            &node_run,
            WorkflowStatus::Succeeded,
            Some(output.to_wire_value()?),
            Some("audit_recorded".into()),
        )
        .await?;
        Ok(ReadyNodeDisposition::Complete)
    }
}
