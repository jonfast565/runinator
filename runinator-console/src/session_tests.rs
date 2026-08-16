//! covers how a cell sees the ones before it.

use super::*;

#[test]
fn earlier_results_are_reachable_under_a_named_scope() {
    let mut context = ConsoleContext::new();
    context.bind("load", runinator_models::json!({ "rows": 3 }));

    // the surface an author writes is `params.load`, but the evaluator reads that reference against
    // a context keyed `input`. the two names differ, and building the context under the surface one
    // produces an expression that looks right and never resolves.
    assert_eq!(CELL_SCOPE, "params");
    assert_eq!(CONTEXT_ROOT, "input");
    let value = context.as_value();
    assert_eq!(
        value.pointer("/input/load/rows").and_then(Value::as_i64),
        Some(3)
    );
    assert!(value.get("load").is_none());

    // and unwrapped for a scratch run's parameters, since the reducer nests them under `params`
    // itself — wrapping twice would make a cell reach `params.params.load`.
    assert_eq!(
        context
            .as_parameters()
            .pointer("/load/rows")
            .and_then(Value::as_i64),
        Some(3)
    );
}

#[test]
fn an_unlabelled_cell_is_still_referenceable_by_position() {
    assert_eq!(cell_binding_name(None, 2), "cell_2");
    assert_eq!(cell_binding_name(Some("load"), 2), "load");
    // whitespace is not a label.
    assert_eq!(cell_binding_name(Some("   "), 2), "cell_2");
}

#[test]
fn a_scratch_workflow_name_is_scoped_to_its_session_and_cell() {
    let session = uuid::Uuid::from_u128(1);
    let cell = uuid::Uuid::from_u128(2);
    let name = scratch_workflow_name(session, cell);

    assert!(name.starts_with("console."));
    // two sessions running identical cell text must not collide on one workflow name.
    assert_ne!(name, scratch_workflow_name(uuid::Uuid::from_u128(3), cell));
    assert_ne!(
        name,
        scratch_workflow_name(session, uuid::Uuid::from_u128(4))
    );
}

#[test]
fn a_fresh_context_still_exposes_the_scope() {
    // so `params.missing` is a missing *field* rather than an unresolvable root, which is the
    // difference between a useful error and a confusing one in an empty session.
    let context = ConsoleContext::new();
    assert!(context.is_empty());
    assert!(context.as_value().get(CONTEXT_ROOT).is_some());
}
