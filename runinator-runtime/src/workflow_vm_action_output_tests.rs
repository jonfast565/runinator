//! Action results remain addressable after durable suspension and graph routing.

use super::*;

fn probe_module() -> runinator_models::workflow_vm::WorkflowModule {
    let reference = runinator_models::json!({
        "$ref": { "node": "probe", "output": ["response", "text"] }
    });
    let mut condition = node("condition", WorkflowNodeKind::Condition, Some("no"));
    condition.transitions.branches = vec![WorkflowBranch {
        when: WorkflowCondition::from_value(runinator_models::json!({
            "value": { "$call": "lower", "args": [
                { "$call": "trim", "args": [reference.clone()] }
            ] },
            "equals": "yes"
        })),
        target: WorkflowNodeRef::new("followup"),
        priority: None,
    }];
    let mut probe = action("probe", Some("condition"), runinator_models::json!({}));
    probe.action.as_mut().unwrap().idempotency_key = None;
    probe.transitions.on_failure = Some(WorkflowNodeRef::new("no"));
    let mut followup = action(
        "followup",
        Some("yes"),
        runinator_models::json!({ "text": reference.clone() }),
    );
    followup.action.as_mut().unwrap().idempotency_key = None;
    compile(
        "action-output-reference",
        vec![
            node("start", WorkflowNodeKind::Start, Some("probe")),
            probe,
            condition,
            followup,
            output("yes", reference),
            output("no", Value::String("no".into())),
            node("end", WorkflowNodeKind::End, None),
        ],
    )
}

#[test]
fn action_output_drives_conditions_effect_inputs_and_final_output_after_reload() {
    let module = probe_module();
    let WorkflowVmStep::Yield {
        continuation: waiting,
        request,
        ..
    } = step_workflow_vm(&module, continuation(&module))
    else {
        panic!("probe must yield")
    };
    let waiting = serde_json::from_value(serde_json::to_value(waiting).unwrap()).unwrap();
    let result = runinator_models::json!({ "response": { "text": " YES\n" } });
    let WorkflowVmStep::Yield {
        continuation: waiting,
        request,
        ..
    } = resume_workflow_vm(&module, waiting, Some(&request), Ok(result.clone()))
    else {
        panic!("successful condition must reach followup")
    };
    assert!(
        matches!(request.as_ref(), WorkflowEffectRequest::Action { input, .. }
        if input["text"] == Value::String(" YES\n".into()))
    );
    assert_eq!(waiting.stack.last(), Some(&result));
    let waiting = serde_json::from_value(serde_json::to_value(waiting).unwrap()).unwrap();
    let WorkflowVmStep::Complete { value, .. } =
        resume_workflow_vm(&module, waiting, Some(&request), Ok(Value::Null))
    else {
        panic!("followup must complete")
    };
    assert_eq!(value, Value::String(" YES\n".into()));
}

#[test]
fn negative_action_output_takes_the_negative_branch() {
    let module = probe_module();
    let WorkflowVmStep::Yield {
        continuation,
        request,
        ..
    } = step_workflow_vm(&module, continuation(&module))
    else {
        panic!("probe must yield")
    };
    let WorkflowVmStep::Complete { value, .. } = resume_workflow_vm(
        &module,
        continuation,
        Some(&request),
        Ok(runinator_models::json!({ "response": { "text": "no\n" } })),
    ) else {
        panic!("negative branch must complete")
    };
    assert_eq!(value, Value::String("no".into()));
}

#[test]
fn failed_action_does_not_bind_a_successful_output() {
    let module = probe_module();
    let WorkflowVmStep::Yield {
        continuation,
        request,
        ..
    } = step_workflow_vm(&module, continuation(&module))
    else {
        panic!("probe must yield")
    };
    let WorkflowVmStep::Complete {
        value,
        continuation,
    } = resume_workflow_vm(
        &module,
        continuation,
        Some(&request),
        Err(WorkflowFailure::new(
            WorkflowFailureKind::Failed,
            "probe failed",
        )),
    )
    else {
        panic!("failure edge must complete")
    };
    assert_eq!(value, Value::String("no".into()));
    assert!(
        !continuation.locals.keys().any(
            |name| name.starts_with(runinator_models::workflow_vm::WORKFLOW_NODE_OUTPUT_PREFIX)
        )
    );
}
