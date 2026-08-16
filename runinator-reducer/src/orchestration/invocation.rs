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

/// the deadline a call gets when neither its `with { }` policy nor its node declares one.
///
/// there is deliberately no "no timeout" case: a call with no deadline is a run that parks forever
/// when a worker dies, which is the failure mode the whole liveness apparatus exists to prevent.
pub(super) const DEFAULT_CALL_TIMEOUT_SECONDS: i64 = 60;

/// name a call's positional arguments with the parameter names its target declares.
///
/// the worker validates an action's parameters against that target's `ActionMetadata` as a closed
/// struct, so a key the metadata does not declare is rejected before the provider ever runs. the
/// positional `arg0`/`arg1` form [`InvocationEffect::to_parameters`] falls back to is therefore only
/// usable for a target with no known signature — for anything the catalog can describe, the
/// arguments have to travel under the names that target actually advertises.
fn call_parameters(catalog: &CallableCatalog, effect: &InvocationEffect) -> Value {
    let name = effect.target.display_name();
    let Some(declared) = catalog
        .resolve(&name)
        .and_then(|entry| entry.signature.as_ref())
        .map(|signature| &signature.parameters)
    else {
        return effect.to_parameters();
    };
    // a call with more arguments than the signature declares is one the type checker should have
    // rejected; naming only some of them would silently drop the rest, so fall back instead.
    if declared.len() < effect.args.len() {
        return effect.to_parameters();
    }
    let mut map = runinator_models::value::Map::new();
    for (parameter, value) in declared.iter().zip(&effect.args) {
        map.insert(parameter.name.clone(), value.clone());
    }
    Value::Object(map)
}

/// whether a dispatched call is past its own deadline.
fn call_expired(call: &WorkflowInvocationCall) -> bool {
    call.deadline_at
        .is_some_and(|deadline| Utc::now().timestamp() > deadline)
}

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
        //
        // a `Queued` run is reused rather than replaced. it means a previous drive created the node
        // run and stopped before marking it `Running` — creating a second one here would leave the
        // first orphaned and give the node two invocations, which
        // `fetch_invocation_for_node_run` would then have to choose between.
        let node_run = match latest.filter(|run| run.status == WorkflowStatus::Queued) {
            Some(node_run) => node_run.clone(),
            None => {
                ctx.db
                    .create_workflow_node_run(
                        ctx.workflow_run.id,
                        ctx.node.id.clone(),
                        ctx.node.parameters.clone().into(),
                        super::context::most_recently_finished_node_run(ctx.node_runs),
                        Some(ctx.cursor),
                    )
                    .await?
            }
        };
        let continuation = runinator_models::invocation::InvocationContinuation::start();
        let invocation = match ctx.db.fetch_invocation_for_node_run(node_run.id).await? {
            Some(invocation) => invocation,
            None => {
                ctx.db
                    .create_invocation(
                        ctx.workflow_run.id,
                        node_run.id,
                        Some(ctx.cursor.id),
                        &ctx.node.id,
                        params.module.version,
                        &continuation,
                    )
                    .await?
            }
        };
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
            // the call is still in flight. the deadline that matters is the *call's*, not the
            // node's: a `do { }` block usually declares no `.timeout()`, and `node.timeout_seconds`
            // being `None` makes `timed_out` permanently false — so a lost worker or a dropped
            // result would park the run forever. every call carries a deadline by construction
            // (`action_for_target` falls back to `DEFAULT_CALL_TIMEOUT_SECONDS`), so this always
            // fires. a node-level timeout, when the author declared one, caps the whole program on
            // top of it.
            if call_expired(call) || transitions::timed_out(ctx.timing(), node_run) {
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
            // keep watching: the executor-claim check is the only prompt dead-worker detection a
            // dispatch has, and it only runs when something drives this node.
            super::action::arm_dispatch_liveness_poll(ctx).await?;
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
                            // the same id the command carries: a store-assigned one would leave the
                            // dispatch naming a call that does not exist.
                            id: call.id,
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
                // no ready node is pending while the call is in flight, so the deadline has to arm
                // its own wake-up. armed from the call's timeout rather than the node's, and
                // unconditionally: `arm_node_timeout` returns without arming when the node declares
                // no timeout, which for a `do { }` block is the common case.
                let seconds = call
                    .deadline_at
                    .map(|deadline| (deadline - Utc::now().timestamp()).max(1))
                    .unwrap_or(DEFAULT_CALL_TIMEOUT_SECONDS);
                transitions::arm_node_timeout_in(ctx, seconds).await?;
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
        // always set: a call with no deadline is a run that parks forever when a worker dies.
        let timeout = if action.timeout_seconds > 0 {
            action.timeout_seconds
        } else {
            DEFAULT_CALL_TIMEOUT_SECONDS
        };
        let deadline_at = Some(Utc::now().timestamp() + timeout);

        let command = ActionCommand {
            command_id: Uuid::new_v4(),
            workflow_run_id: ctx.workflow_run.id,
            workflow_node_run_id: node_run.id,
            node_id: ctx.node.id.clone(),
            action,
            // an invocation's attempts are per call, not per node run, so the command carries the
            // call's attempt. this is the first attempt of a newly recorded call.
            attempt: 0,
            parameters: call_parameters(&CallableCatalog::builtin(), effect),
            target: runinator_comm::ActionTarget::Any,
            trace_id: Uuid::now_v7(),
            trace_context: runinator_utilities::telemetry::current_trace_context(),
            notification_delivery_id: None,
            invocation_call_id: Some(call_id),
            idempotency_key: idempotency_key.clone(),
        };
        Ok(PreparedCall {
            id: call_id,
            command,
            idempotency_key,
            deadline_at,
        })
    }
}

/// what `dispatch` prepared: the command plus the fields the call row also needs.
struct PreparedCall {
    /// the id the command names, which the call row must be written under.
    id: Uuid,
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
            .or(node.timeout_seconds)
            .unwrap_or(DEFAULT_CALL_TIMEOUT_SECONDS),
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
