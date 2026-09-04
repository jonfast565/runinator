//! Replay planning and reconstruction from frozen bytecode and durable receipts only.
use runinator_models::{
    errors::SendableError,
    replay::*,
    value::Value,
    workflow_vm::{
        WorkflowContinuation, WorkflowContinuationStatus, WorkflowEffectRequest,
        WorkflowEffectStatus, WorkflowFrame, WorkflowJournalEntry,
    },
    workflows::WorkflowNodeKind,
};
use runinator_runtime::workflow_vm::{WorkflowVmStep, step_to_replay_checkpoint};
use runinator_store::{
    RuntimeStore,
    roles::{NewWorkflowVmRun, WorkflowVmStore, workflow_vm::WorkflowReplaySeed},
};
use std::collections::{BTreeMap, BTreeSet};
use uuid::Uuid;

pub async fn replay_plan<T: RuntimeStore + WorkflowVmStore>(
    db: &T,
    id: Uuid,
    from: Option<String>,
) -> Result<ReplayPlan, SendableError> {
    Ok(prepare(db, id, from).await?.0)
}

pub async fn replay_with_options<T: RuntimeStore + WorkflowVmStore>(
    db: &T,
    id: Uuid,
    options: ReplayOptions,
) -> Result<runinator_models::workflows::WorkflowRun, SendableError> {
    let (plan, start) = prepare(db, id, options.from_step_id.clone()).await?;
    validate_plan(&plan, &options)?;
    let start =
        start.ok_or_else(|| crate::errors::REPLAY_UNSAFE.error("reconstruction is unavailable"))?;
    db.create_workflow_vm_run(start).await
}

pub fn validate_plan(plan: &ReplayPlan, options: &ReplayOptions) -> Result<(), SendableError> {
    if plan.verdict == ReplayVerdict::Blocked {
        return Err(crate::errors::REPLAY_UNSAFE.error(plan.reasons.join("; ")));
    }
    if options
        .plan_fingerprint
        .as_ref()
        .is_some_and(|fingerprint| fingerprint != &plan.plan_fingerprint)
        || (plan.verdict == ReplayVerdict::Review
            && (!options.acknowledge_review
                || options.plan_fingerprint.as_ref() != Some(&plan.plan_fingerprint)))
    {
        return Err(crate::errors::REPLAY_UNSAFE
            .error("fetch a fresh replay plan and explicitly acknowledge its review"));
    }
    Ok(())
}

async fn prepare<T: RuntimeStore + WorkflowVmStore>(
    db: &T,
    id: Uuid,
    from: Option<String>,
) -> Result<(ReplayPlan, Option<NewWorkflowVmRun>), SendableError> {
    let source = db
        .fetch_workflow_run(id)
        .await?
        .ok_or_else(|| crate::errors::REPLAY_NOT_FOUND.error(id))?;
    let mut plan = ReplayPlan {
        source_run_id: id,
        from_step_id: from.clone(),
        workflow_snapshot: source.workflow_snapshot.clone(),
        seeded_receipts: Vec::new(),
        actions: Vec::new(),
        reasons: Vec::new(),
        verdict: ReplayVerdict::Safe,
        plan_fingerprint: String::new(),
    };
    let Some(snapshot) = source.workflow_snapshot.clone() else {
        return Ok(block(
            plan,
            "the original frozen workflow snapshot is unavailable",
        ));
    };
    let Some(module) = db.fetch_workflow_module(id).await? else {
        return Ok(block(
            plan,
            "the original frozen workflow module is unavailable",
        ));
    };
    if !module.is_supported() {
        return Ok(block(plan, "the frozen module version is unsupported"));
    }
    let roots = db
        .fetch_workflow_continuations(id)
        .await?
        .into_iter()
        .filter(|c| c.parent_id.is_none())
        .collect::<Vec<_>>();
    if roots.len() != 1 {
        return Ok(block(
            plan,
            "a unique original root continuation is required",
        ));
    }
    let root = &roots[0];
    if !root.is_supported() {
        return Ok(block(
            plan,
            "the original continuation version is unsupported",
        ));
    }
    let Some(config) = root.locals.get("config").cloned() else {
        return Ok(block(plan, "the frozen configuration is unavailable"));
    };
    let effects = db.fetch_workflow_effects(id).await?;
    let journal = db.fetch_workflow_journal(id).await?;
    let locations = journal
        .iter()
        .filter_map(|entry| match &entry.entry {
            WorkflowJournalEntry::EffectRequested {
                effect_id,
                instruction_pointer: Some(ip),
            } => module
                .graph_location(*ip)
                .map(|location| (*effect_id, location.node_id.clone())),
            _ => None,
        })
        .collect::<BTreeMap<_, _>>();
    let mut seeded_nodes = BTreeSet::new();
    let mut seed = None;
    let mut ip = 0;
    if let Some(target) = &from {
        if let Err(error) = super::debug::ancestors_in_snapshot(&snapshot, target) {
            return Ok(block(plan, &error.to_string()));
        }
        let entries = module
            .source_map
            .iter()
            .filter(|entry| &entry.node_id == target && entry.edge_label.is_none())
            .collect::<Vec<_>>();
        if entries.len() != 1 {
            return Ok(block(
                plan,
                "restart point has no unique frozen source-map boundary",
            ));
        }
        ip = entries[0].instruction_start;
        // local time, environment, and random identities cannot be reconstructed from receipts.
        // conservatively scan the complete module, including functions reachable indirectly.
        if !module.interrupt_handlers.is_empty()
            || has_local_intrinsic(&serde_json::to_value(&module)?)
        {
            return Ok(block(
                plan,
                "selected-step replay cannot reconstruct interrupt or nondeterministic local state",
            ));
        }
        let mut continuation = WorkflowContinuation::start(id, module.version);
        continuation
            .locals
            .insert("input".into(), source.parameters.clone());
        continuation.locals.insert("config".into(), config.clone());
        let mut seen = BTreeSet::new();
        loop {
            match step_to_replay_checkpoint(&module, continuation, ip) {
                WorkflowVmStep::Joined {
                    continuation: at,
                    join_key,
                    ..
                } if join_key == "replay-checkpoint" => {
                    if at
                        .frames
                        .iter()
                        .any(|frame| !matches!(frame, WorkflowFrame::Debug(_)))
                    {
                        return Ok(block(plan, "restart requires stateful continuation frames"));
                    }
                    seeded_nodes.extend(at.pending_node_entries.iter().cloned());
                    seed = Some(WorkflowReplaySeed {
                        locals: at.locals,
                        stack: at.stack,
                    });
                    break;
                }
                WorkflowVmStep::Yield {
                    continuation: mut at,
                    sequence,
                    request,
                    ..
                } => {
                    let node = module
                        .graph_location(at.instruction_pointer.saturating_sub(1))
                        .map(|entry| entry.node_id.clone());
                    let Some(node) = node else {
                        return Ok(block(plan, "ancestor effect has no source location"));
                    };
                    if !seen.insert(node.clone())
                        || at
                            .frames
                            .iter()
                            .any(|f| !matches!(f, WorkflowFrame::Debug(_)))
                    {
                        return Ok(block(
                            plan,
                            "repeated or stateful ancestor cannot be seeded",
                        ));
                    }
                    // only completed ordinary actions may be bypassed. Leases, approvals, child
                    // runs and other orchestration effects must retain their original lifecycle.
                    if !matches!(*request, WorkflowEffectRequest::Action { .. }) {
                        return Ok(block(
                            plan,
                            "ancestor is an orchestration effect, not a seedable action",
                        ));
                    }
                    let receipts = effects
                        .iter()
                        .filter(|e| {
                            e.continuation_id == root.id
                                && e.sequence == sequence
                                && locations.get(&e.id) == Some(&node)
                        })
                        .collect::<Vec<_>>();
                    if receipts.len() != 1 {
                        return Ok(block(plan, "ancestor has no unique durable receipt"));
                    }
                    let receipt = receipts[0];
                    if receipt.status != WorkflowEffectStatus::Succeeded
                        || receipt.result.is_none()
                        || receipt.request != *request
                        || !receipt.is_supported()
                    {
                        return Ok(block(
                            plan,
                            "ancestor receipt is incomplete, failed, or does not match reconstructed inputs",
                        ));
                    }
                    plan.seeded_receipts.push(ReplaySeedReceipt {
                        node_id: node.clone(),
                        effect_id: receipt.id,
                        attempt: receipt.attempt,
                    });
                    seeded_nodes.extend(at.pending_node_entries.iter().cloned());
                    at.pending_node_entries.clear();
                    at.stack.push(receipt.result.clone().unwrap_or(Value::Null));
                    at.awaiting_effect_id = None;
                    at.status = WorkflowContinuationStatus::Runnable;
                    continuation = at;
                }
                WorkflowVmStep::Failed { message, .. } => {
                    return Ok(block(
                        plan,
                        &format!("prefix reconstruction failed: {message}"),
                    ));
                }
                _ => {
                    return Ok(block(
                        plan,
                        "recorded path does not reach the restart point without stateful control flow",
                    ));
                }
            }
        }
    }
    for node in &snapshot.definition.nodes {
        if seeded_nodes.contains(&node.id) {
            continue;
        }
        for action in node.action.iter().chain(node.compensation.iter()) {
            let keys = effects
                .iter()
                .filter(|effect| locations.get(&effect.id) == Some(&node.id))
                .filter_map(|effect| match &effect.request {
                    WorkflowEffectRequest::Action {
                        idempotency_key,
                        provider,
                        function,
                        ..
                    } if provider == &action.provider && function == &action.function => {
                        idempotency_key.clone()
                    }
                    _ => None,
                })
                .collect();
            plan.actions.push(ReplayAction {
                node_id: node.id.clone(), provider: action.provider.clone(), function: action.function.clone(),
                declared_idempotency_key: action.idempotency_key.clone(), previous_resolved_idempotency_keys: keys,
                reason: if action.idempotency_key.is_some() { "a declared key is not a guarantee: values, reservation retention, and provider behavior may differ" } else { "no external idempotency key is declared; duplicate side effects are possible" }.into(),
            });
        }
        if !matches!(
            node.kind,
            WorkflowNodeKind::Start
                | WorkflowNodeKind::End
                | WorkflowNodeKind::Fail
                | WorkflowNodeKind::Output
                | WorkflowNodeKind::Condition
                | WorkflowNodeKind::Switch
                | WorkflowNodeKind::Toggle
                | WorkflowNodeKind::Percentage
                | WorkflowNodeKind::Assert
                | WorkflowNodeKind::Transform
                | WorkflowNodeKind::Action
        ) {
            plan.reasons.push(format!(
                "{} may repeat {:?} orchestration or control-flow behavior",
                node.id, node.kind
            ));
        }
    }
    // bytecode is authoritative even for compute calls and older graph/module projections.
    for (location, instruction) in module.instructions.iter().enumerate() {
        let node_id = module
            .graph_location(location)
            .map(|entry| entry.node_id.as_str())
            .unwrap_or("<module>");
        if seeded_nodes.contains(node_id) {
            continue;
        }
        inspect_calls(node_id, &serde_json::to_value(instruction)?, &mut plan);
        if matches!(
            instruction,
            runinator_models::workflow_vm::WorkflowInstruction::Effect { .. }
        ) {
            plan.verdict = ReplayVerdict::Review;
        }
    }
    if !plan.actions.is_empty() || !plan.reasons.is_empty() || plan.verdict == ReplayVerdict::Review
    {
        plan.verdict = ReplayVerdict::Review;
        plan.reasons.push(
            "actions are a conservative may-execute list; branches and compensation may not run"
                .into(),
        );
    }
    // bind acknowledgement to receipt content, frozen code, inputs and reconstruction, not time.
    plan.plan_fingerprint = ReplayPlan::fingerprint(&serde_json::to_vec(&(
        &plan,
        &module,
        &effects,
        &config,
        &source.parameters,
    ))?);
    let state = runinator_models::json!({ "debug": { "enabled": true, "mode": "breakpoints" },
        "replay": { "source_run_id": id, "from_step_id": from, "seeded_receipts": plan.seeded_receipts, "plan_fingerprint": plan.plan_fingerprint }
    });
    let start = NewWorkflowVmRun {
        workflow_id: source.workflow_id,
        workflow_snapshot: snapshot,
        parameters: source.parameters,
        config,
        state,
        name: source.name,
        replay_seed: seed,
        module,
        instruction_pointer: ip,
        pipeline_run_id: None,
        pipeline_member_attempt_id: None,
        provenance: runinator_models::replicas::WorkflowRunProvenance {
            source_kind: Some(runinator_models::replicas::TriggerSourceKind::Replay),
            actor_type: Some(runinator_models::replicas::TriggerActorType::System),
            actor_display_name: Some("replay".into()),
            metadata: runinator_models::json!({ "source_run_id": id, "seeded_receipts": plan.seeded_receipts, "plan_fingerprint": plan.plan_fingerprint }),
            ..Default::default()
        },
    };
    Ok((plan, Some(start)))
}

fn inspect_calls(node_id: &str, value: &serde_json::Value, plan: &mut ReplayPlan) {
    if let Some(object) = value.as_object() {
        let kind = object.get("kind").and_then(|v| v.as_str());
        let action = object.get("type").and_then(|v| v.as_str()) == Some("action");
        if (kind == Some("provider") || action)
            && let (Some(provider), Some(function)) = (
                object.get("provider").and_then(|v| v.as_str()),
                object.get("function").and_then(|v| v.as_str()),
            )
            && !plan
                .actions
                .iter()
                .any(|a| a.node_id == node_id && a.provider == provider && a.function == function)
        {
            plan.actions.push(ReplayAction {
                node_id: node_id.into(),
                provider: provider.into(),
                function: function.into(),
                declared_idempotency_key: object.get("idempotency_key").cloned().map(Value::from),
                previous_resolved_idempotency_keys: Vec::new(),
                reason: "frozen bytecode may invoke this action; external safety is not guaranteed"
                    .into(),
            });
        }
        if kind == Some("packaged")
            || kind == Some("intrinsic")
                && object
                    .get("name")
                    .and_then(|v| v.as_str())
                    .is_some_and(|name| {
                        !runinator_compute::CallableCatalog::builtin()
                            .resolve(name)
                            .is_some_and(|entry| entry.is_in_process())
                    })
        {
            plan.reasons.push(format!(
                "{node_id} may invoke an effectful compiled callable"
            ));
        }
        for nested in object.values() {
            inspect_calls(node_id, nested, plan);
        }
    } else if let Some(values) = value.as_array() {
        for nested in values {
            inspect_calls(node_id, nested, plan);
        }
    }
}

fn block(mut plan: ReplayPlan, reason: &str) -> (ReplayPlan, Option<NewWorkflowVmRun>) {
    plan.verdict = ReplayVerdict::Blocked;
    plan.reasons.push(reason.into());
    (plan, None)
}

fn has_local_intrinsic(value: &serde_json::Value) -> bool {
    match value {
        serde_json::Value::String(name) => {
            runinator_compute::LOCAL_INTRINSIC_NAMES.contains(&name.as_str())
        }
        serde_json::Value::Array(values) => values.iter().any(has_local_intrinsic),
        serde_json::Value::Object(values) => values.values().any(has_local_intrinsic),
        _ => false,
    }
}

#[cfg(test)]
#[path = "replay_tests.rs"]
mod tests;
