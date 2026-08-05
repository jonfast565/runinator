//! exhaustive node-kind conformance for the author-time layer.
//!
//! the per-kind `match` statements in this crate are exhaustive, so the compiler already forces a
//! new `WorkflowNodeKind` to be handled in validation, typing, parameters, and simulation. what the
//! compiler cannot check is whether the *catalog's own default template* for a kind produces a node
//! that survives those paths — that is, whether dragging the kind out of the command center palette
//! yields a workflow that validates. these tests close that gap by driving every kind from its
//! catalog entry.

use runinator_models::catalog_metadata::{EdgeTaxonomy, LocationBase};
use runinator_models::value::{Map, Value};
use runinator_models::workflows::{WorkflowDefinition, WorkflowNodeKind};

use crate::{catalog::node_kind_catalog, validate_workflow};

const SUBJECT: &str = "subject";
/// a runnable, non-terminal node that control-flow bodies can legally point at.
const FILLER: &str = "filler";

/// kinds whose default template is deliberately incomplete: they fan out to, or delegate to,
/// content only the author can supply, so an unedited template cannot validate.
///
/// each entry names what the author must add. this list is the point of the test — a kind added
/// here is a decision that shows up in review, and every kind *not* here is guaranteed to validate
/// straight out of the palette.
const REQUIRES_AUTHORING: &[(WorkflowNodeKind, &str)] = &[
    (WorkflowNodeKind::Percentage, "at least one weighted bucket"),
    (WorkflowNodeKind::Parallel, "at least one branch"),
    (WorkflowNodeKind::Join, "at least one wait_for branch"),
    (WorkflowNodeKind::Race, "at least one branch"),
    (
        WorkflowNodeKind::Subflow,
        "a subflow_id or subflow.workflow_name",
    ),
];

/// a node reference as it is written in workflow json.
fn node_ref(id: &str) -> Value {
    let mut object = Map::new();
    object.insert("$node".into(), Value::String(id.to_string()));
    Value::Object(object)
}

/// write `value` at `path` inside `root`, creating intermediate objects as needed.
fn write_at(root: &mut Value, path: &[String], value: Value) {
    let Some((last, parents)) = path.split_last() else {
        *root = value;
        return;
    };
    let mut cursor = root;
    for segment in parents {
        if !matches!(cursor, Value::Object(_)) {
            *cursor = Value::Object(Map::new());
        }
        let Value::Object(object) = cursor else {
            return;
        };
        cursor = object
            .entry(segment.clone())
            .or_insert_with(|| Value::Object(Map::new()));
    }
    if !matches!(cursor, Value::Object(_)) {
        *cursor = Value::Object(Map::new());
    }
    if let Value::Object(object) = cursor {
        object.insert(last.clone(), value);
    }
}

fn base_key(base: &LocationBase) -> Option<&'static str> {
    match base {
        LocationBase::Parameters => Some("parameters"),
        LocationBase::Wait => Some("wait"),
        LocationBase::Condition => Some("condition"),
        LocationBase::Action => Some("action"),
        LocationBase::Transitions => Some("transitions"),
        LocationBase::TopLevel => None,
    }
}

/// build a minimal `start -> <kind> -> end` graph from a kind's catalog default template, wiring
/// every control-flow edge slot the catalog declares to `end`.
///
/// this mirrors what the command center does when a node is dropped on the canvas: it reads the
/// same `default_template` and the same `edge_slots`, and has no other per-kind knowledge.
fn workflow_for(kind: WorkflowNodeKind) -> Option<WorkflowDefinition> {
    let catalog = node_kind_catalog();
    let entry = catalog.iter().find(|item| item.kind == kind)?;

    let mut template = entry.default_template.clone();
    write_at(
        &mut template,
        &["id".to_string()],
        Value::String(SUBJECT.into()),
    );

    for slot in &entry.edge_slots {
        // branch/predicate slots carry `{ when, target }` records rather than a bare reference;
        // leaving them empty is valid, and validation is what we are exercising here.
        if slot.multiple {
            continue;
        }
        // a control edge names a body to run (try/map/loop); it must point at a runnable node.
        // direct edges are ordinary outcomes and may terminate the run.
        let target = match slot.taxonomy {
            EdgeTaxonomy::Control => node_ref(FILLER),
            EdgeTaxonomy::Direct | EdgeTaxonomy::Branch => node_ref("end"),
        };
        match base_key(&slot.target.base) {
            Some(base) => {
                let mut path = vec![base.to_string()];
                path.extend(slot.target.path.iter().cloned());
                write_at(&mut template, &path, target);
            }
            None => write_at(&mut template, &slot.target.path, target),
        }
    }

    // the happy-path edge, written under both keys so next-based and on_success-based leaves are
    // both satisfied without this test needing to know which a kind uses.
    for key in ["next", "on_success"] {
        write_at(
            &mut template,
            &["transitions".to_string(), key.to_string()],
            node_ref("end"),
        );
    }

    let graph = runinator_models::json!({
        "start": "start",
        "nodes": [
            {
                "id": "start",
                "kind": "start",
                "transitions": { "next": { "$node": SUBJECT } }
            },
            template,
            {
                "id": FILLER,
                "kind": "action",
                "action": { "provider": "console", "function": "run", "configuration": {} },
                "transitions": { "on_success": { "$node": "end" } }
            },
            { "id": "end", "kind": "end" }
        ]
    });

    Some(
        serde_json::from_value(
            runinator_models::json!({
                "id": "00000000-0000-0000-0000-000000000001",
                "name": format!("conformance {kind:?}"),
                "version": "1.0.0",
                "enabled": true,
                "definition": graph
            })
            .into(),
        )
        .unwrap_or_else(|error| panic!("{kind:?} template did not build a definition: {error}")),
    )
}

/// every kind in the model has a catalog entry.
///
/// duplicated deliberately from `tests.rs` in the negative direction: that test asserts the catalog
/// has no gaps, this one is the precondition the rest of this file relies on.
#[test]
fn every_node_kind_has_a_catalog_entry() {
    let catalog = node_kind_catalog();
    for kind in WorkflowNodeKind::ALL {
        assert!(
            catalog.iter().any(|item| item.kind == kind),
            "{kind:?} has no catalog entry, so the palette cannot offer it"
        );
    }
}

/// every addable kind's catalog default template validates when wired into a minimal graph.
///
/// a failure here means the palette hands the user a node that the backend rejects.
#[test]
fn every_addable_kind_default_template_validates() {
    let mut unexpected_failures = Vec::new();
    let mut unexpected_passes = Vec::new();

    for entry in node_kind_catalog().into_iter().filter(|item| item.addable) {
        let Some(workflow) = workflow_for(entry.kind.clone()) else {
            continue;
        };
        let needs_authoring = REQUIRES_AUTHORING
            .iter()
            .find(|(kind, _)| *kind == entry.kind);

        match (validate_workflow(&workflow), needs_authoring) {
            (Err(error), None) => {
                unexpected_failures.push(format!("{:?}: {error}", entry.kind));
            }
            // the template now validates unedited, so its `REQUIRES_AUTHORING` entry is stale.
            (Ok(_), Some((kind, needs))) => {
                unexpected_passes.push(format!("{kind:?} (listed as needing {needs})"));
            }
            _ => {}
        }
    }

    assert!(
        unexpected_failures.is_empty(),
        "catalog default templates do not validate, so the palette offers a node the backend \
         rejects:\n  {}",
        unexpected_failures.join("\n  ")
    );
    assert!(
        unexpected_passes.is_empty(),
        "these kinds now validate unedited; remove them from REQUIRES_AUTHORING:\n  {}",
        unexpected_passes.join("\n  ")
    );
}
