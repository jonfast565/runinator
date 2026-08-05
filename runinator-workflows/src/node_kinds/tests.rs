//! conformance for the node-kind registry itself.
//!
//! the registry's `match` is exhaustive, so the compiler guarantees every kind has *a* spec. what
//! it cannot check is that each arm points at the *right* spec, or that a kind's catalog edge slots
//! and its extracted target slots describe the same edges. both of those are copy-paste failures
//! the type system is blind to, and the second is the drift these tests exist to stop: the catalog
//! advertising an edge the graph walkers never see.

use runinator_models::catalog_metadata::{EdgeTaxonomy, LocationBase, NodeEdgeSlot};
use runinator_models::value::{Map, Value};
use runinator_models::workflows::{WorkflowNode, WorkflowNodeKind};

use super::{spec_for, target_slots};

/// control edge slots whose elements carry more than a bare node reference, with the shape the
/// seeder should write. the catalog describes *where* a slot's targets live but not what rides
/// alongside them, so these two need the shape spelled out.
///
/// a slot listed here that turns out to accept a bare node reference fails
/// [`typed_element_shapes_are_still_needed`], so the table cannot quietly go stale.
const TYPED_ELEMENT_SHAPES: &[(WorkflowNodeKind, &str, &str)] = &[
    (
        WorkflowNodeKind::Switch,
        "cases",
        r#"[{ "target": { "$node": "body" }, "equals": "x" }]"#,
    ),
    (
        WorkflowNodeKind::Percentage,
        "buckets",
        r#"[{ "weight": 1, "target": { "$node": "body" } }]"#,
    ),
];

fn node_ref(id: &str) -> Value {
    let mut object = Map::new();
    object.insert("$node".into(), Value::String(id.to_string()));
    Value::Object(object)
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

/// the element shape a slot needs, when a bare node reference is not enough.
fn typed_shape(kind: &WorkflowNodeKind, slot: &NodeEdgeSlot) -> Option<Value> {
    TYPED_ELEMENT_SHAPES
        .iter()
        .find(|(listed, key, _)| listed == kind && *key == slot.key)
        .map(|(_, _, shape)| {
            serde_json::from_str::<serde_json::Value>(shape)
                .expect("typed element shape is not valid json")
                .into()
        })
}

/// write a slot's target into `template`, at the location the catalog declares.
fn seed_slot(template: &mut Value, slot: &NodeEdgeSlot, target: Value) {
    match base_key(&slot.target.base) {
        Some(base) => {
            let mut path = vec![base.to_string()];
            path.extend(slot.target.path.iter().cloned());
            write_at(template, &path, target);
        }
        None => write_at(template, &slot.target.path, target),
    }
}

/// build a node from a kind's default template with every control edge slot pointed at `body`,
/// mirroring what the command center writes when an author wires an edge.
fn seeded_node(kind: &WorkflowNodeKind) -> Option<WorkflowNode> {
    let metadata = spec_for(kind).metadata();
    let mut template = metadata.default_template.clone();
    if !matches!(template, Value::Object(_)) {
        return None;
    }
    write_at(
        &mut template,
        &["id".to_string()],
        Value::String("subject".into()),
    );

    for slot in &metadata.edge_slots {
        if slot.taxonomy != EdgeTaxonomy::Control {
            continue;
        }
        let target = match typed_shape(kind, slot) {
            Some(shape) => shape,
            None if slot.multiple => Value::Array(vec![node_ref("body")]),
            None => node_ref("body"),
        };
        seed_slot(&mut template, slot, target);
    }

    serde_json::from_value(template.into()).ok()
}

/// every dispatch arm points at the spec for the kind it is keyed on.
///
/// the registry is 35 near-identical arms; a mis-wired one would otherwise surface as a node kind
/// silently behaving like its neighbour.
#[test]
fn every_dispatch_arm_returns_its_own_spec() {
    for kind in WorkflowNodeKind::ALL {
        let spec = spec_for(&kind);
        assert_eq!(
            spec.kind(),
            kind,
            "spec_for({kind:?}) returned the spec for {:?}",
            spec.kind()
        );
        assert_eq!(
            spec.metadata().kind,
            kind,
            "{kind:?}'s catalog entry is labelled {:?}",
            spec.metadata().kind
        );
    }
}

/// every control edge slot the catalog advertises is an edge the graph walkers actually see.
///
/// this is the direction that catches dead metadata: a slot the palette lets an author wire, that
/// validation, cycle detection, and map-body isolation never read. a `loop` node advertised a
/// `target` slot for exactly this reason — the reducer routes a loop body through
/// `transitions.next` and never reads `parameters.target`.
#[test]
fn every_declared_control_edge_is_extracted() {
    let mut missing = Vec::new();
    for kind in WorkflowNodeKind::ALL {
        let metadata = spec_for(&kind).metadata();
        let seedable: Vec<&NodeEdgeSlot> = metadata
            .edge_slots
            .iter()
            .filter(|slot| slot.taxonomy == EdgeTaxonomy::Control)
            .collect();
        if seedable.is_empty() {
            continue;
        }
        let Some(node) = seeded_node(&kind) else {
            missing.push(format!("{kind:?}: default template did not build a node"));
            continue;
        };
        let extracted = match target_slots(&node) {
            Ok(slots) => slots,
            Err(error) => {
                missing.push(format!("{kind:?}: target extraction failed: {error}"));
                continue;
            }
        };
        for slot in seedable {
            if !extracted.iter().any(|found| found.key == slot.key) {
                missing.push(format!(
                    "{kind:?} declares control edge '{}' but target_slots never yields it, so the \
                     graph walkers cannot see that edge",
                    slot.key
                ));
            }
        }
    }
    assert!(missing.is_empty(), "{}", missing.join("\n  "));
}

/// every extracted target slot corresponds to a control edge the catalog advertises.
///
/// the opposite drift: an edge the graph enforces that the palette gives no way to author.
#[test]
fn every_extracted_target_is_declared() {
    let mut undeclared = Vec::new();
    for kind in WorkflowNodeKind::ALL {
        let metadata = spec_for(&kind).metadata();
        let Some(node) = seeded_node(&kind) else {
            continue;
        };
        let Ok(extracted) = target_slots(&node) else {
            continue;
        };
        for found in extracted {
            let declared = metadata
                .edge_slots
                .iter()
                .any(|slot| slot.taxonomy == EdgeTaxonomy::Control && slot.key == found.key);
            if !declared {
                undeclared.push(format!(
                    "{kind:?} extracts target slot '{}' that the catalog does not advertise, so the \
                     palette offers no way to author it",
                    found.key
                ));
            }
        }
    }
    assert!(undeclared.is_empty(), "{}", undeclared.join("\n  "));
}

/// the typed-element table stays honest: a listed slot that a bare node reference can populate no
/// longer needs its hand-written shape, and a slot that no longer exists should be dropped.
#[test]
fn typed_element_shapes_are_still_needed() {
    let mut stale = Vec::new();
    for (kind, key, _) in TYPED_ELEMENT_SHAPES {
        let metadata = spec_for(kind).metadata();
        let Some(slot) = metadata.edge_slots.iter().find(|slot| slot.key == *key) else {
            stale.push(format!(
                "{kind:?} no longer declares a '{key}' edge slot; drop it from \
                 TYPED_ELEMENT_SHAPES"
            ));
            continue;
        };
        let mut template = metadata.default_template.clone();
        write_at(
            &mut template,
            &["id".to_string()],
            Value::String("subject".into()),
        );
        seed_slot(&mut template, slot, Value::Array(vec![node_ref("body")]));

        let parsed: Option<WorkflowNode> = serde_json::from_value(template.into()).ok();
        if parsed
            .as_ref()
            .and_then(|node| target_slots(node).ok())
            .is_some_and(|slots| slots.iter().any(|found| found.key == *key))
        {
            stale.push(format!(
                "{kind:?}.{key} now accepts a bare node reference; drop its hand-written shape \
                 from TYPED_ELEMENT_SHAPES"
            ));
        }
    }
    assert!(stale.is_empty(), "{}", stale.join("\n  "));
}

/// a terminal kind is terminal in both the palette and the graph walkers.
///
/// `base()` stamps the palette's flag from the graph role, so this pins that wiring rather than a
/// hand-kept second list.
#[test]
fn terminal_metadata_matches_the_graph_role() {
    for kind in WorkflowNodeKind::ALL {
        let spec = spec_for(&kind);
        assert_eq!(
            spec.metadata().terminal,
            spec.graph_role().terminal,
            "{kind:?} disagrees with itself about whether it settles the run"
        );
    }
}

/// only `start`, `end`, and `fail` are barred from being a branch or body target.
#[test]
fn only_the_graph_endpoints_are_unrunnable() {
    for kind in WorkflowNodeKind::ALL {
        let expected = !matches!(
            kind,
            WorkflowNodeKind::Start | WorkflowNodeKind::End | WorkflowNodeKind::Fail
        );
        assert_eq!(
            spec_for(&kind).graph_role().runnable_entry,
            expected,
            "{kind:?} has the wrong runnable_entry role"
        );
    }
}
