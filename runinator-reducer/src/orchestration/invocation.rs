//! the `invocation` node: run a compiled program, suspending on each durable call it makes.
//!
//! the shape that makes this different from every other handler is that **one node run spans many
//! dispatches**. an action node dispatches once and settles; an invocation node stays `Running`
//! while its program yields a call, waits for the result, resumes, and yields again — possibly many
//! times — and only settles when the program itself finishes. that is what keeps retries, logs and
//! artifacts attributed to the node the author wrote instead of to a synthetic node per call.
//!
//! every drive of this node re-enters the same loop: step the vm as far as it will go in process,
//! then do exactly one of four things with what comes back. the loop is bounded by
//! `MAX_CALLS_PER_DRIVE` because a program that yields is *supposed* to leave the reducer — a drive
//! that dispatched an unbounded number of calls would hold its claim while doing so.

use super::context::runtime_context;
use super::*;

use runinator_compute::{CallableCatalog, VmEnv};
use runinator_models::invocation::{
    CallPolicy, CallableTarget, InvocationEffect, InvocationEffectResult, InvocationModule,
    InvocationStep, NewInvocationCall, WorkflowInvocation, WorkflowInvocationCall,
};
use runinator_workflows::parse_invocation_parameters;

pub(super) struct InvocationHandler;

impl<T: ReducerStore> super::handler::NodeHandler<T> for InvocationHandler {
    async fn process<'a>(
        &'a self,
        ctx: &'a super::handler::NodeHandlerContext<'a, T>,
    ) -> Result<ReadyNodeDisposition, SendableError>
    where
        T: 'a,
    {
        let params = parse_invocation_parameters(ctx.node)
            .map_err(|err| -> SendableError { Box::new(err) })?;

        // a loop body re-entering this node sees the prior iteration's terminal run; treat it as a
        // fresh visit so the program runs again instead of transitioning from the stale run.
        let latest = ctx
            .latest
            .filter(|run| !super::context::is_reentry_stale(run, ctx.node_runs, ctx.cursor));

        if let Some(node_run) = latest {
            if node_run.status == WorkflowStatus::Running {
                return self.drive_running(ctx, node_run, &params.module).await;
            }
            if node_run.status.is_terminal() {
                transitions::retry_or_transition(
                    ctx,
                    node_run,
                    node_run.status,
                    node_run.output_json.clone(),
                    node_run.message.clone(),
                )
                .await?;
                return Ok(ReadyNodeDisposition::Complete);
            }
        }

        // a fresh visit: one node run and one invocation, positioned at the start of the module.
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
        let continuation = runinator_models::invocation::InvocationContinuation::start();
        let invocation = ctx
            .db
            .create_invocation(
                ctx.workflow_run.id,
                node_run.id,
                Some(ctx.cursor.id),
                &ctx.node.id,
                params.module.version,
                &continuation,
            )
            .await?;
        ctx.db
            .update_workflow_node_run(
                node_run.id,
                WorkflowStatus::Running,
                Some(node_run.attempt + 1),
                None,
                None,
                None,
                Some("invocation_started".into()),
                None,
            )
            .await?;

        // the vm environment borrows a non-`Sync` library slot, so it must not be live across an
        // await or the handler future stops being `Send`. scoping it here is what keeps `advance`
        // reachable.
        let step = {
            let context = runtime_context(ctx).await;
            let catalog = CallableCatalog::builtin();
            runinator_compute::start(&params.module, &VmEnv::pure(&context, &catalog))
        };
        self.advance(ctx, &node_run, &invocation, step).await
    }
}

impl InvocationHandler {
    /// a drive that found the node already running: either its call has landed, or it has not.
    async fn drive_running<T: ReducerStore>(
        &self,
        ctx: &super::handler::NodeHandlerContext<'_, T>,
        node_run: &WorkflowNodeRun,
        module: &InvocationModule,
    ) -> Result<ReadyNodeDisposition, SendableError> {
        let Some(invocation) = ctx.db.fetch_invocation_for_node_run(node_run.id).await? else {
            // a running node run with no invocation row cannot make progress and cannot be
            // diagnosed by waiting. fail it so the node's own failure transition decides.
            return super::handler::complete(
                transitions::settle_node(
                    ctx,
                    node_run,
                    WorkflowStatus::Failed,
                    None,
                    Some("invocation state is missing for this node run".into()),
                    false,
                )
                .await
                .map(|_| ()),
            );
        };

        let pending = ctx.db.fetch_pending_invocation_call(invocation.id).await?;
        if let Some(call) = &pending {
            // the call is still in flight. honour the node timeout so a lost worker or a dropped
            // result cannot park the run forever.
            if transitions::timed_out(ctx.timing(), node_run) {
                let message = format!(
                    "Invocation call '{}' did not return before the node timeout elapsed",
                    call.target.display_name()
                );
                ctx.db
                    .settle_invocation_call(
                        call.id,
                        call.attempt,
                        WorkflowStatus::TimedOut,
                        None,
                        Some(message.clone()),
                    )
                    .await?;
                ctx.db
                    .settle_invocation(
                        invocation.id,
                        WorkflowStatus::TimedOut,
                        None,
                        Some(message.clone()),
                    )
                    .await?;
                return super::handler::complete(
                    transitions::settle_node(
                        ctx,
                        node_run,
                        WorkflowStatus::TimedOut,
                        None,
                        Some(message),
                        true,
                    )
                    .await
                    .map(|_| ()),
                );
            }
            return Ok(ReadyNodeDisposition::Complete);
        }

        // no open call: the most recent one settled, so resume the program with its outcome.
        let calls = ctx.db.fetch_invocation_calls(invocation.id).await?;
        let Some(last) = calls.last() else {
            // nothing has been called yet — a drive that arrived between creating the invocation
            // and reaching the first call. re-step from the stored continuation.
            let step = {
                let context = runtime_context(ctx).await;
                let catalog = CallableCatalog::builtin();
                runinator_compute::step(
                    module,
                    invocation.continuation.clone(),
                    &VmEnv::pure(&context, &catalog),
                )
            };
            return self.advance(ctx, node_run, &invocation, step).await;
        };

        let result = match last.status {
            WorkflowStatus::Succeeded => InvocationEffectResult::Ok {
                value: last.result.clone().unwrap_or(Value::Null),
            },
            _ => InvocationEffectResult::Failed {
                message: last.message.clone().unwrap_or_else(|| {
                    format!("call '{}' did not succeed", last.target.display_name())
                }),
            },
        };
        let step = {
            let context = runtime_context(ctx).await;
            let catalog = CallableCatalog::builtin();
            runinator_compute::resume(
                module,
                invocation.continuation.clone(),
                result,
                &VmEnv::pure(&context, &catalog),
            )
        };
        self.advance(ctx, node_run, &invocation, step).await
    }

    /// apply one vm step.
    ///
    /// exactly one drive per durable call, by construction: a yield returns here, and the program
    /// cannot proceed until the call lands, so there is nothing a loop would do. the vm's own
    /// instruction budget is what bounds the *in-process* work one step performs.
    async fn advance<T: ReducerStore>(
        &self,
        ctx: &super::handler::NodeHandlerContext<'_, T>,
        node_run: &WorkflowNodeRun,
        invocation: &WorkflowInvocation,
        step: InvocationStep,
    ) -> Result<ReadyNodeDisposition, SendableError> {
        match step {
            InvocationStep::Complete { value } => {
                ctx.db
                    .settle_invocation(
                        invocation.id,
                        WorkflowStatus::Succeeded,
                        Some(value.clone()),
                        None,
                    )
                    .await?;
                transitions::transition_from_node(
                    ctx,
                    node_run,
                    WorkflowStatus::Succeeded,
                    Some(value),
                    Some("invocation_completed".into()),
                )
                .await?;
                return Ok(ReadyNodeDisposition::Complete);
            }
            InvocationStep::Failed { message } => {
                ctx.db
                    .settle_invocation(
                        invocation.id,
                        WorkflowStatus::Failed,
                        None,
                        Some(message.clone()),
                    )
                    .await?;
                return super::handler::complete(
                    transitions::settle_node(
                        ctx,
                        node_run,
                        WorkflowStatus::Failed,
                        None,
                        Some(message),
                        true,
                    )
                    .await
                    .map(|_| ()),
                );
            }
            InvocationStep::Goto { target } => {
                let target = resolve_goto_target(&target, ctx.nodes);
                ctx.db
                    .settle_invocation(invocation.id, WorkflowStatus::Succeeded, None, None)
                    .await?;
                ctx.db
                    .update_workflow_node_run(
                        node_run.id,
                        WorkflowStatus::Succeeded,
                        Some(node_run.attempt + 1),
                        None,
                        Some(Value::Null),
                        None,
                        Some("invocation_goto".into()),
                        None,
                    )
                    .await?;
                // `goto` moves this thread of control, so it moves the cursor. writing only
                // `active_node_id` would leave the cursor here and the drive would spin.
                run_state::advance_cursor(
                    ctx.db,
                    ctx.workflow_run.id,
                    ctx.cursor.id,
                    WorkflowStatus::Running,
                    run_state::CursorMove::To(target),
                    None,
                )
                .await?;
                return Ok(ReadyNodeDisposition::Complete);
            }
            InvocationStep::Yield {
                effect,
                continuation,
            } => {
                let call = self.dispatch(ctx, node_run, &effect).await?;
                ctx.db
                    .suspend_invocation(
                        &continuation,
                        NewInvocationCall {
                            invocation_id: invocation.id,
                            workflow_run_id: ctx.workflow_run.id,
                            sequence: effect.sequence,
                            target: effect.target.clone(),
                            arguments: effect.args.clone(),
                            policy: effect.policy.clone(),
                            idempotency_key: call.idempotency_key.clone(),
                            deadline_at: call.deadline_at,
                        },
                        call.command,
                    )
                    .await?;
                // the deadline belongs to the node run, which is what the timeout check above
                // reads; no ready node is pending while the call is in flight, so it has to arm
                // its own wake-up.
                transitions::arm_node_timeout(ctx).await?;
                Ok(ReadyNodeDisposition::Complete)
            }
        }
    }

    /// build the dispatch for one yielded call.
    async fn dispatch<T: ReducerStore>(
        &self,
        ctx: &super::handler::NodeHandlerContext<'_, T>,
        node_run: &WorkflowNodeRun,
        effect: &InvocationEffect,
    ) -> Result<PreparedCall, SendableError> {
        let action = action_for_target(&effect.target, &effect.policy, ctx.node);
        let call_id = Uuid::now_v7();
        let idempotency_key = match &effect.policy.idempotency_key {
            Some(expression) => {
                let context = runtime_context(ctx).await;
                runinator_workflows::resolve_value_refs(expression, &context)
                    .ok()
                    .as_ref()
                    .and_then(Value::as_str)
                    .filter(|key| !key.is_empty())
                    .map(str::to_string)
            }
            None => None,
        };
        let deadline_at = action
            .timeout_seconds
            .gt(&0)
            .then(|| Utc::now().timestamp() + action.timeout_seconds);

        let command = ActionCommand {
            command_id: Uuid::new_v4(),
            workflow_run_id: ctx.workflow_run.id,
            workflow_node_run_id: node_run.id,
            node_id: ctx.node.id.clone(),
            action,
            // an invocation's attempts are per call, not per node run, so the command carries the
            // call's attempt. this is the first attempt of a newly recorded call.
            attempt: 0,
            parameters: effect.to_parameters(),
            target: runinator_comm::ActionTarget::Any,
            trace_id: Uuid::now_v7(),
            trace_context: runinator_utilities::telemetry::current_trace_context(),
            notification_delivery_id: None,
            invocation_call_id: Some(call_id),
            idempotency_key: idempotency_key.clone(),
        };
        Ok(PreparedCall {
            command,
            idempotency_key,
            deadline_at,
        })
    }
}

/// what `dispatch` prepared: the command plus the two fields the call row also needs.
struct PreparedCall {
    command: ActionCommand,
    idempotency_key: Option<String>,
    deadline_at: Option<i64>,
}

/// the provider action one callable target dispatches as.
///
/// an intrinsic becomes a direct `std.<name>` action rather than a whole program: the point of the
/// ir is that the reducer already knows which single function it wants, so shipping a program plus
/// the run context — which is what the compute path did — would send everything the worker no
/// longer needs to decide anything.
fn action_for_target(
    target: &CallableTarget,
    policy: &CallPolicy,
    node: &WorkflowNode,
) -> WorkflowAction {
    let (provider, function, binding) = match target {
        CallableTarget::Intrinsic { name } => ("std".to_string(), name.clone(), None),
        CallableTarget::Local { name } => {
            // a module function is called in process; reaching here means one was classified
            // durable, which the compiler should have prevented.
            ("std".to_string(), name.clone(), None)
        }
        CallableTarget::Provider { provider, function } => {
            (provider.clone(), function.clone(), None)
        }
        CallableTarget::Packaged { binding } => (
            "functions".to_string(),
            "invoke".to_string(),
            Some(binding.clone()),
        ),
    };
    WorkflowAction {
        provider,
        function,
        timeout_seconds: policy
            .timeout_seconds
            .or_else(|| node.action.as_ref().map(|action| action.timeout_seconds))
            .unwrap_or(60),
        configuration: Default::default(),
        mcp_enabled: false,
        tags: policy.tags.clone(),
        required_labels: policy
            .runner
            .as_ref()
            .map(|runner| {
                let mut labels = std::collections::BTreeMap::new();
                labels.insert("type".to_string(), runner.clone());
                labels
            })
            .unwrap_or_default(),
        idempotency_key: None,
        function_binding: binding,
    }
}

// resolve a goto target: a real node id is used directly; the synthetic `done`/`fail` map to the
// workflow's end/fail node ids.
fn resolve_goto_target(target: &str, nodes: &[WorkflowNode]) -> String {
    if nodes.iter().any(|node| node.id == target) {
        return target.to_string();
    }
    let kind = match target {
        "done" => Some(WorkflowNodeKind::End),
        "fail" => Some(WorkflowNodeKind::Fail),
        _ => None,
    };
    if let Some(kind) = kind
        && let Some(node) = nodes.iter().find(|node| node.kind == kind)
    {
        return node.id.clone();
    }
    target.to_string()
}

/// settle the call a landed result names, and report whether it was applied.
///
/// this is the engine-facing half: a result carrying an `invocation_call_id` settles that call
/// rather than the node run, and a drive of the owning cursor is what resumes the program. a call
/// already terminal returns `false`, which is how a duplicate or superseded delivery is discarded.
pub async fn settle_call<T: ReducerStore>(
    db: &T,
    call: &WorkflowInvocationCall,
    status: WorkflowStatus,
    output: Option<Value>,
    message: Option<String>,
) -> Result<bool, SendableError> {
    db.settle_invocation_call(call.id, call.attempt, status, output, message)
        .await
}
