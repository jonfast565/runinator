//! parallel interpreter regressions.

use super::*;

#[test]
fn fork_makes_independently_addressable_children() {
    let module = WorkflowModule::new(vec![
        WorkflowInstruction::Fork {
            targets: vec![1, 2],
            join_key: "all".into(),
        },
        WorkflowInstruction::Return,
        WorkflowInstruction::Return,
    ]);
    let WorkflowVmStep::Fork {
        parent, children, ..
    } = step(&module, continuation())
    else {
        panic!("expected fork");
    };
    assert_eq!(parent.status, WorkflowContinuationStatus::Joined);
    assert_eq!(children.len(), 2);
    assert_ne!(children[0].id, children[1].id);
    assert!(
        children
            .iter()
            .all(|child| child.parent_id == Some(parent.id))
    );
}

#[test]
fn duplicate_fork_drive_preserves_child_identity_and_order() {
    let module = WorkflowModule::new(vec![WorkflowInstruction::Fork {
        targets: vec![1, 2],
        join_key: "all".into(),
    }]);
    let continuation = continuation();
    let WorkflowVmStep::Fork {
        children: first, ..
    } = step(&module, continuation.clone())
    else {
        panic!("expected fork");
    };
    let WorkflowVmStep::Fork {
        children: second, ..
    } = step(&module, continuation)
    else {
        panic!("expected fork");
    };
    assert_eq!(
        first.iter().map(|child| child.id).collect::<Vec<_>>(),
        second.iter().map(|child| child.id).collect::<Vec<_>>()
    );
}

#[test]
fn race_forks_deterministically_and_keeps_winner_state_only_on_the_parent() {
    let module = WorkflowModule::new(vec![WorkflowInstruction::Race {
        targets: vec![1, 2],
        race_key: "fastest".into(),
        winner: runinator_models::workflow_vm::WorkflowBranchPolicy::FirstSuccess,
    }]);
    let WorkflowVmStep::Fork {
        parent,
        children,
        join_key,
    } = step(&module, continuation())
    else {
        panic!("race should fork contenders");
    };
    assert_eq!(join_key, "fastest");
    assert!(parent.frames.iter().any(|frame| matches!(
        frame,
        WorkflowFrame::Race(frame)
            if frame.expected == 2
                && frame.winner_policy == runinator_models::workflow_vm::WorkflowBranchPolicy::FirstSuccess
                && frame.winner.is_none()
    )));
    assert!(children.iter().all(|child| {
        child.fork_key.as_deref() == Some("fastest")
            && child
                .frames
                .iter()
                .any(|frame| matches!(frame, WorkflowFrame::Fork(_)))
            && !child
                .frames
                .iter()
                .any(|frame| matches!(frame, WorkflowFrame::Race(_)))
    }));
}

#[test]
fn map_forks_only_its_concurrency_window_with_stable_item_bindings() {
    let module = WorkflowModule::new(vec![
        WorkflowInstruction::Const {
            value: Value::Array(vec![1.into(), 2.into(), 3.into()]),
        },
        WorkflowInstruction::BeginMap {
            map_key: "fanout".into(),
            body: 3,
            exit: 4,
            concurrency: 2,
        },
    ]);
    let WorkflowVmStep::Fork {
        parent,
        children,
        join_key,
    } = step(&module, continuation())
    else {
        panic!("map should fork its initial window");
    };
    assert_eq!(join_key, "fanout");
    assert_eq!(children.len(), 2);
    assert_eq!(children[0].locals.get("fanout.item"), Some(&Value::from(1)));
    assert_eq!(
        children[1].locals.get("fanout.index"),
        Some(&Value::from(1))
    );
    assert!(parent.frames.iter().any(|frame| matches!(
        frame, WorkflowFrame::Map(frame)
            if frame.next_index == 2 && frame.items.len() == 3 && frame.results.is_empty()
    )));
    assert!(
        children
            .iter()
            .all(|child| child.frames.iter().any(|frame| matches!(
                frame, WorkflowFrame::Map(frame) if frame.item_index.is_some()
            )))
    );
}

#[test]
fn map_child_arrival_reports_its_body_result_not_the_re_evaluated_items() {
    let module = WorkflowModule::new(vec![
        WorkflowInstruction::Const {
            value: Value::Array(vec![1.into()]),
        },
        WorkflowInstruction::BeginMap {
            map_key: "fanout".into(),
            body: 2,
            exit: 5,
            concurrency: 1,
        },
        WorkflowInstruction::Const {
            value: Value::String("body output".into()),
        },
        WorkflowInstruction::Const {
            value: Value::Array(vec![1.into()]),
        },
        WorkflowInstruction::Jump { target: 1 },
        WorkflowInstruction::Return,
    ]);
    let WorkflowVmStep::Fork { children, .. } = step(&module, continuation()) else {
        panic!("map should fork its item");
    };
    let WorkflowVmStep::Joined {
        join_key, value, ..
    } = step(&module, children.into_iter().next().unwrap())
    else {
        panic!("map child should arrive at the map coordinator");
    };
    assert_eq!(join_key, "fanout");
    assert_eq!(value, Value::String("body output".into()));
}
