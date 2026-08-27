//! covers interrupt handler regions: the isolation rules and the handler-safe allowlist.

use super::*;

/// a graph whose main flow is start → wait → end, plus whatever handler nodes and `interrupts`
/// metadata the test supplies.
fn with_handler(
    interrupts: runinator_models::value::Value,
    handler_nodes: Vec<runinator_models::value::Value>,
) -> WorkflowDefinition {
    let mut nodes = vec![
        runinator_models::json!({
            "id": "start", "kind": "start",
            "transitions": { "next": { "$node": "poll" } }
        }),
        runinator_models::json!({
            "id": "poll", "kind": "wait", "wait": { "seconds": 60 },
            "transitions": { "next": { "$node": "end" } }
        }),
        runinator_models::json!({ "id": "end", "kind": "end" }),
    ];
    nodes.extend(handler_nodes);
    workflow(runinator_models::json!({
        "start": "start",
        "nodes": nodes,
        "metadata": { "interrupts": interrupts },
    }))
}

/// the ordinary shape: a handler region of one action, terminated by a resume.
fn refresh_region() -> Vec<runinator_models::value::Value> {
    vec![
        runinator_models::json!({
            "id": "refresh", "kind": "action",
            "action": { "provider": "std", "function": "noop" },
            "transitions": { "on_success": { "$node": "handled" } }
        }),
        runinator_models::json!({
            "id": "handled", "kind": "resume", "parameters": { "mode": "resume" }
        }),
    ]
}

/// the same region with an explicit, source-neutral structural entry.
fn graph_declared_region() -> Vec<runinator_models::value::Value> {
    let mut nodes = vec![runinator_models::json!({
        "id": "on_wake", "kind": "interrupt",
        "transitions": { "next": { "$node": "refresh" } }
    })];
    nodes.extend(refresh_region());
    nodes
}

fn declarations_of(workflow: &WorkflowDefinition) -> Vec<(String, String)> {
    let (_, nodes) = parse_nodes(workflow).expect("the fixture parses");
    interrupt_declarations(workflow, &nodes)
        .into_iter()
        .map(|declaration| (declaration.on, declaration.handler))
        .collect()
}

fn expect_region_error(workflow: &WorkflowDefinition) -> String {
    match validate_workflow(workflow) {
        Err(WorkflowValidationError::InterruptHandlerNotIsolatable { reason, .. }) => reason,
        other => panic!("expected a region error, got {other:?}"),
    }
}

#[test]
fn a_well_formed_handler_region_validates() {
    let workflow = with_handler(
        runinator_models::json!([{ "on": "wake", "handler": "refresh" }]),
        refresh_region(),
    );
    validate_workflow(&workflow).expect("an isolated handler region is valid");
}

#[test]
fn multiple_timer_handlers_with_distinct_intervals_validate() {
    let workflow = with_handler(
        runinator_models::json!([
            { "on": "timer", "handler": "fast", "interval_seconds": 30 },
            { "on": "timer", "handler": "slow", "interval_seconds": 300 }
        ]),
        vec![
            runinator_models::json!({
                "id": "fast", "kind": "interrupt",
                "transitions": { "next": { "$node": "fast_resume" } }
            }),
            runinator_models::json!({
                "id": "fast_resume", "kind": "resume",
                "parameters": { "mode": "resume" }
            }),
            runinator_models::json!({
                "id": "slow", "kind": "interrupt",
                "transitions": { "next": { "$node": "slow_resume" } }
            }),
            runinator_models::json!({
                "id": "slow_resume", "kind": "resume",
                "parameters": { "mode": "resume" }
            }),
        ],
    );
    validate_workflow(&workflow).expect("separate timer declarations may run at separate periods");
}

#[test]
fn timers_with_a_shared_handler_get_distinct_frozen_ids() {
    let workflow = with_handler(
        runinator_models::json!([
            { "on": "timer", "handler": "refresh", "interval_seconds": 30 },
            { "on": "timer", "handler": "refresh", "interval_seconds": 300 }
        ]),
        refresh_region(),
    );

    let module = compile_workflow_module(&workflow).expect("the shared handler is still valid");
    let timers: Vec<_> = module
        .interrupt_handlers
        .iter()
        .filter(|handler| handler.source == runinator_models::interrupt::InterruptSource::Timer)
        .collect();
    assert_eq!(
        timers
            .iter()
            .map(|handler| handler.interval_seconds)
            .collect::<Vec<_>>(),
        vec![Some(30), Some(300)]
    );
    assert_ne!(timers[0].timer_id, timers[1].timer_id);
}

#[test]
fn a_timer_handler_requires_a_positive_interval() {
    let workflow = with_handler(
        runinator_models::json!([{ "on": "timer", "handler": "on_wake" }]),
        graph_declared_region(),
    );
    assert!(expect_region_error(&workflow).contains("positive `interval_seconds`"));
}

/// metadata links a source to an explicit graph entry.
#[test]
fn a_region_declared_by_its_entry_node_validates() {
    let workflow = with_handler(
        runinator_models::json!([{ "on": "wake", "handler": "on_wake" }]),
        graph_declared_region(),
    );
    validate_workflow(&workflow).expect("a metadata-linked graph region is valid");
    assert_eq!(
        declarations_of(&workflow),
        vec![("wake".to_string(), "on_wake".to_string())]
    );
}

/// metadata owns the source and enabled state for a graph region.
#[test]
fn metadata_controls_the_graph_region_link() {
    let workflow = with_handler(
        runinator_models::json!([{ "on": "timeout", "handler": "on_wake", "enabled": false }]),
        graph_declared_region(),
    );
    validate_workflow(&workflow).expect("a disabled metadata link still validates its region");
    assert_eq!(
        declarations_of(&workflow),
        vec![("timeout".to_string(), "on_wake".to_string())]
    );
}

/// a definition written before the entry node existed still declares its handler through metadata.
#[test]
fn metadata_still_declares_a_handler_when_the_graph_does_not() {
    let workflow = with_handler(
        runinator_models::json!([{ "on": "wake", "handler": "refresh" }]),
        refresh_region(),
    );
    assert_eq!(
        declarations_of(&workflow),
        vec![("wake".to_string(), "refresh".to_string())]
    );
}

/// an interrupt entry is an entry point, so no ordinary transition may route into it.
#[test]
fn a_transition_into_an_interrupt_node_is_rejected() {
    let mut nodes = graph_declared_region();
    nodes.push(runinator_models::json!({
        "id": "stray", "kind": "audit", "parameters": { "action": "x" },
        "transitions": { "next": { "$node": "on_wake" } }
    }));
    let workflow = with_handler(runinator_models::json!([]), nodes);
    match validate_workflow(&workflow) {
        Err(WorkflowValidationError::InvalidNodeReferenceType { target, .. }) => {
            assert_eq!(target, "on_wake");
        }
        other => panic!("expected a reference-type error, got {other:?}"),
    }
}

/// nor may a body or branch target one — the rule `runnable_entry` alone would have let through.
#[test]
fn a_map_body_targeting_an_interrupt_node_is_rejected() {
    let mut nodes = graph_declared_region();
    nodes.push(runinator_models::json!({
        "id": "fan", "kind": "map",
        "parameters": { "items": [], "target": { "$node": "on_wake" }, "concurrency": 2 },
        "transitions": { "next": { "$node": "end" } }
    }));
    let workflow = with_handler(runinator_models::json!([]), nodes);
    match validate_workflow(&workflow) {
        Err(WorkflowValidationError::InvalidNodeReferenceType { target, .. }) => {
            assert_eq!(target, "on_wake");
        }
        other => panic!("expected a reference-type error, got {other:?}"),
    }
}

/// an unknown source stays fail-open: it loads, and the runtime simply never matches it.
///
#[test]
fn an_unknown_interrupt_source_still_validates() {
    let workflow = with_handler(
        runinator_models::json!([{ "on": "from_a_newer_binary", "handler": "on_wake" }]),
        graph_declared_region(),
    );
    validate_workflow(&workflow).expect("an unknown source must not fail the whole definition");
}

/// an interrupt entry with no metadata link is malformed.
#[test]
fn an_unlinked_interrupt_entry_is_rejected() {
    let workflow = with_handler(runinator_models::json!([]), graph_declared_region());
    assert!(expect_region_error(&workflow).contains("not linked by metadata"));
}

/// metadata links are preserved, and legacy body links are migrated to explicit entries.
#[test]
fn normalization_preserves_metadata_and_materializes_legacy_entries() {
    let workflow = with_handler(
        runinator_models::json!([{ "on": "timeout", "handler": "on_wake", "enabled": false }]),
        graph_declared_region(),
    );
    let normalized = normalize_workflow(&workflow);
    assert_eq!(
        normalized.definition.metadata.get("interrupts"),
        Some(&runinator_models::json!([{
            "on": "timeout", "handler": "on_wake", "enabled": false
        }])),
        "metadata remains the source of the link and enabled state"
    );

    let legacy = with_handler(
        runinator_models::json!([{ "on": "wake", "handler": "refresh" }]),
        refresh_region(),
    );
    let normalized = normalize_workflow(&legacy);
    assert_eq!(
        declarations_of(&normalized),
        vec![("wake".into(), "__interrupt_0_entry".into())],
        "metadata-only declarations gain a graph-visible entry"
    );
    validate_workflow(&normalized).expect("the migrated legacy region remains valid");
}

/// the save path normalizes before it validates, so the region must survive normalization.
///
/// this is the shape that used to break: a `resume` carries no success transition, so
/// `route_success_terminals_to_end` wired it to `end`, which dragged a node that is not
/// `handler_safe` into the region. validating the raw definition never saw it.
#[test]
fn a_handler_region_survives_normalization() {
    let workflow = with_handler(
        runinator_models::json!([{ "on": "wake", "handler": "refresh" }]),
        refresh_region(),
    );
    let normalized = normalize_workflow(&workflow);
    validate_workflow(&normalized).expect("an isolated handler region survives normalization");
}

#[test]
fn a_workflow_with_no_interrupts_is_untouched() {
    let workflow = with_handler(runinator_models::json!([]), Vec::new());
    validate_workflow(&workflow).expect("the feature costs nothing when unused");
}

/// an unknown source must not fail the parse — a definition from a newer binary still loads, and
/// the runtime simply never matches a source it cannot name.
#[test]
fn an_unknown_source_still_validates_its_region() {
    let workflow = with_handler(
        runinator_models::json!([{ "on": "webhook", "handler": "refresh" }]),
        refresh_region(),
    );
    validate_workflow(&workflow).expect("an unknown source is ignored, not rejected");
}

#[test]
fn a_handler_naming_a_missing_node_is_rejected() {
    let workflow = with_handler(
        runinator_models::json!([{ "on": "wake", "handler": "nowhere" }]),
        Vec::new(),
    );
    assert!(expect_region_error(&workflow).contains("does not exist"));
}

#[test]
fn a_region_reachable_from_start_is_rejected() {
    // the main flow falls into the handler entry, so the region is not entered only by its
    // interrupt.
    let mut nodes = refresh_region();
    nodes[0] = runinator_models::json!({
        "id": "refresh", "kind": "action",
        "action": { "provider": "std", "function": "noop" },
        "transitions": { "on_success": { "$node": "handled" } }
    });
    let workflow = workflow(runinator_models::json!({
        "start": "start",
        "nodes": [
            { "id": "start", "kind": "start", "transitions": { "next": { "$node": "refresh" } } },
            { "id": "end", "kind": "end" },
            nodes[0], nodes[1],
        ],
        "metadata": { "interrupts": [{ "on": "wake", "handler": "refresh" }] },
    }));
    let reason = expect_region_error(&workflow);
    assert!(
        reason.contains("handler is reachable from the workflow start"),
        "the main flow must never be able to wander into a handler: {reason}"
    );
}

#[test]
fn a_region_containing_a_parking_kind_is_rejected() {
    let workflow = with_handler(
        runinator_models::json!([{ "on": "wake", "handler": "refresh" }]),
        vec![
            runinator_models::json!({
                "id": "refresh", "kind": "signal", "parameters": { "name": "late" },
                "transitions": { "on_success": { "$node": "handled" } }
            }),
            runinator_models::json!({ "id": "handled", "kind": "resume" }),
        ],
    );
    let reason = expect_region_error(&workflow);
    assert!(
        reason.contains("Signal") && reason.contains("not supported"),
        "a parking kind would pin the suspended thread open: {reason}"
    );
}

#[test]
fn a_region_containing_a_forking_kind_is_rejected() {
    let workflow = with_handler(
        runinator_models::json!([{ "on": "wake", "handler": "refresh" }]),
        vec![
            runinator_models::json!({
                "id": "refresh", "kind": "parallel",
                "parameters": { "branches": [{ "$node": "handled" }] },
                "transitions": { "on_success": { "$node": "handled" } }
            }),
            runinator_models::json!({ "id": "handled", "kind": "resume" }),
        ],
    );
    assert!(expect_region_error(&workflow).contains("not supported"));
}

#[test]
fn a_region_containing_a_terminal_is_rejected() {
    let workflow = with_handler(
        runinator_models::json!([{ "on": "wake", "handler": "refresh" }]),
        vec![runinator_models::json!({
            "id": "refresh", "kind": "action",
            "action": { "provider": "std", "function": "noop" },
            "transitions": { "on_success": { "$node": "end" } }
        })],
    );
    let reason = expect_region_error(&workflow);
    assert!(
        reason.contains("'end' is a End node") && reason.contains("not supported"),
        "a region must return control, not settle the run: {reason}"
    );
}

#[test]
fn a_region_that_never_reaches_a_resume_is_rejected() {
    let workflow = with_handler(
        runinator_models::json!([{ "on": "wake", "handler": "refresh" }]),
        vec![runinator_models::json!({
            "id": "refresh", "kind": "action",
            "action": { "provider": "std", "function": "noop" }
        })],
    );
    assert!(expect_region_error(&workflow).contains("never reaches a resume"));
}

#[test]
fn the_main_flow_may_not_enter_a_region() {
    let workflow = with_handler(
        runinator_models::json!([{ "on": "wake", "handler": "refresh" }]),
        vec![
            runinator_models::json!({
                "id": "refresh", "kind": "action",
                "action": { "provider": "std", "function": "noop" },
                "transitions": { "on_success": { "$node": "handled" } }
            }),
            runinator_models::json!({ "id": "handled", "kind": "resume" }),
            // a stray node outside the region that jumps into it.
            runinator_models::json!({
                "id": "stray", "kind": "action",
                "action": { "provider": "std", "function": "noop" },
                "transitions": { "on_success": { "$node": "refresh" } }
            }),
        ],
    );
    assert!(expect_region_error(&workflow).contains("enters the region"));
}

#[test]
fn two_handlers_may_not_share_a_source() {
    let workflow = with_handler(
        runinator_models::json!([
            { "on": "wake", "handler": "refresh" },
            { "on": "wake", "handler": "handled" },
        ]),
        refresh_region(),
    );
    assert!(expect_region_error(&workflow).contains("one handler per source"));
}

#[test]
fn an_unknown_resume_mode_is_rejected() {
    let workflow = with_handler(
        runinator_models::json!([{ "on": "wake", "handler": "refresh" }]),
        vec![
            runinator_models::json!({
                "id": "refresh", "kind": "action",
                "action": { "provider": "std", "function": "noop" },
                "transitions": { "on_success": { "$node": "handled" } }
            }),
            runinator_models::json!({
                "id": "handled", "kind": "resume", "parameters": { "mode": "explode" }
            }),
        ],
    );
    match validate_workflow(&workflow) {
        Err(WorkflowValidationError::InvalidNodeParameters { message, .. }) => {
            assert!(message.contains("unknown resume mode"), "{message}");
        }
        other => panic!("expected a parameter error, got {other:?}"),
    }
}

/// the two new graph facts must agree with the reasons the plan recorded for them.
#[test]
fn the_handler_allowlist_is_opt_in_and_excludes_parking_and_forking_kinds() {
    use crate::node_kinds::graph_role;

    for allowed in [
        WorkflowNodeKind::Action,
        WorkflowNodeKind::Condition,
        WorkflowNodeKind::Switch,
        WorkflowNodeKind::Toggle,
        WorkflowNodeKind::Percentage,
        WorkflowNodeKind::Transform,
        WorkflowNodeKind::Assert,
        WorkflowNodeKind::Audit,
        WorkflowNodeKind::Output,
        WorkflowNodeKind::Config,
        WorkflowNodeKind::Resume,
        // the region's own entry node is a member of the region the walk collects.
        WorkflowNodeKind::Interrupt,
    ] {
        assert!(
            graph_role(&allowed).handler_safe,
            "{allowed:?} is on the v1 allowlist"
        );
    }

    for excluded in [
        // parks: would pin the suspended thread open indefinitely.
        WorkflowNodeKind::Wait,
        WorkflowNodeKind::Signal,
        WorkflowNodeKind::Approval,
        WorkflowNodeKind::Gate,
        WorkflowNodeKind::Mutex,
        // forks: cursors with no handler to belong to.
        WorkflowNodeKind::Parallel,
        WorkflowNodeKind::Race,
        WorkflowNodeKind::Map,
        WorkflowNodeKind::Join,
        // unbounded or child-run shaped.
        WorkflowNodeKind::Loop,
        WorkflowNodeKind::Try,
        WorkflowNodeKind::Subflow,
        // region terminals must be `resume`.
        WorkflowNodeKind::Start,
        WorkflowNodeKind::End,
        WorkflowNodeKind::Fail,
    ] {
        assert!(
            !graph_role(&excluded).handler_safe,
            "{excluded:?} must stay off the allowlist until it is deliberately supported"
        );
    }
}

#[test]
fn the_graph_endpoints_and_join_cannot_be_interrupted() {
    use crate::node_kinds::graph_role;

    for kind in [
        WorkflowNodeKind::Start,
        WorkflowNodeKind::End,
        WorkflowNodeKind::Fail,
        WorkflowNodeKind::Resume,
        WorkflowNodeKind::Join,
    ] {
        assert!(!graph_role(&kind).interruptible, "{kind:?}");
    }
    assert!(graph_role(&WorkflowNodeKind::Action).interruptible);
    assert!(graph_role(&WorkflowNodeKind::Wait).interruptible);
}
