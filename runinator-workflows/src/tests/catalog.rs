//! the backend-driven catalog the ui reads: every kind present, default templates that deserialize,
//! and field/edge locations that actually exist in those templates.

use super::*;

#[test]
fn node_kind_catalog_covers_every_kind() {
    let catalog = node_kind_catalog();
    assert_eq!(catalog.len(), WorkflowNodeKind::ALL.len());
    for kind in WorkflowNodeKind::ALL {
        let entry = catalog
            .iter()
            .find(|item| item.kind == kind)
            .unwrap_or_else(|| panic!("missing catalog entry for {kind:?}"));
        assert!(!entry.label.is_empty(), "{kind:?} needs a label");
        assert!(!entry.icon.is_empty(), "{kind:?} needs an icon");
        assert!(
            !entry.description.is_empty(),
            "{kind:?} needs a description"
        );
        assert!(!entry.category.is_empty(), "{kind:?} needs a category");
    }
}

#[test]
fn node_kind_catalog_default_templates_deserialize_to_nodes() {
    // every addable kind's default template must round-trip into a real WorkflowNode (with an id).
    for entry in node_kind_catalog().into_iter().filter(|item| item.addable) {
        let mut template = entry.default_template.clone();
        if let runinator_models::value::Value::Object(object) = &mut template {
            object.insert(
                "id".into(),
                runinator_models::value::Value::String("n1".into()),
            );
        }
        let node: WorkflowNode = serde_json::from_value(serde_json::to_value(&template).unwrap())
            .unwrap_or_else(|error| {
                panic!("{:?} template is not a valid node: {error}", entry.kind)
            });
        assert_eq!(
            node.kind, entry.kind,
            "template kind mismatch for {:?}",
            entry.kind
        );
    }
}

#[test]
fn loop_and_map_items_field_is_typed_array() {
    // the iterable of a loop/map advertises Array<Any> (not the bare Any it used to be) so the
    // editor and wdl completion can flow the element type into the loop-body variable.
    let catalog = node_kind_catalog();
    for kind in [WorkflowNodeKind::Loop, WorkflowNodeKind::Map] {
        let entry = catalog
            .iter()
            .find(|item| item.kind == kind)
            .unwrap_or_else(|| panic!("missing catalog entry for {kind:?}"));
        let items = entry
            .fields
            .iter()
            .find(|f| f.field.param.name == "items")
            .unwrap_or_else(|| panic!("{kind:?} needs an items field"));
        assert_eq!(
            items.field.param.ty,
            runinator_models::types::RuninatorType::array(
                runinator_models::types::RuninatorType::Any
            ),
            "{kind:?} items must be Array<Any>"
        );
        // the expression widget is kept: loops iterate an upstream reference, not a literal array.
        assert_eq!(items.field.widget.as_deref(), Some("expression"));
    }
}

#[test]
fn trigger_catalog_covers_every_kind() {
    let catalog = trigger_kind_catalog();
    assert_eq!(catalog.len(), WorkflowTriggerKind::ALL.len());
    for kind in WorkflowTriggerKind::ALL {
        assert!(
            catalog.iter().any(|item| item.kind == kind),
            "missing trigger {kind:?}"
        );
    }
}

#[test]
fn enum_catalog_covers_expected_enums() {
    let catalog = enum_catalogs();
    let expected = ["gate_kind", "match_kind", "branch_policy", "setting_kind"];

    assert_eq!(catalog.len(), expected.len());
    for name in expected {
        let entry = catalog
            .iter()
            .find(|item| item.name == name)
            .unwrap_or_else(|| panic!("missing enum catalog {name}"));
        assert!(!entry.options.is_empty(), "{name} needs options");
        for option in &entry.options {
            assert!(
                !option.value.is_empty(),
                "{name} has an option without a value"
            );
            assert!(
                !option.label.is_empty(),
                "{name} has an option without a label"
            );
        }
    }

    let values = |name: &str| {
        catalog
            .iter()
            .find(|item| item.name == name)
            .unwrap()
            .options
            .iter()
            .map(|option| option.value.as_str())
            .collect::<Vec<_>>()
    };
    assert_eq!(values("gate_kind"), ["manual", "condition", "external"]);
    assert_eq!(values("branch_policy"), ["all", "any", "first_success"]);
    assert_eq!(values("setting_kind"), ["config", "secret"]);
    assert_eq!(
        values("match_kind"),
        ["equals", "not_equals", "exists", "when"]
    );
}

#[test]
fn node_kind_catalog_protected_kinds_are_not_addable() {
    for kind in [
        WorkflowNodeKind::Start,
        WorkflowNodeKind::End,
        WorkflowNodeKind::Fail,
    ] {
        let entry = node_kind_catalog()
            .into_iter()
            .find(|item| item.kind == kind)
            .unwrap_or_else(|| panic!("missing catalog entry for {kind:?}"));
        assert!(entry.protected, "{kind:?} must be protected");
        assert!(!entry.addable, "{kind:?} must not be addable");
    }
}

#[test]
fn node_kind_catalog_terminal_kinds_have_no_outgoing_slots() {
    for kind in [WorkflowNodeKind::End, WorkflowNodeKind::Fail] {
        let entry = node_kind_catalog()
            .into_iter()
            .find(|item| item.kind == kind)
            .unwrap_or_else(|| panic!("missing catalog entry for {kind:?}"));
        assert!(entry.terminal, "{kind:?} must be terminal");
        assert!(
            entry.edge_slots.is_empty(),
            "{kind:?} must not expose edge slots"
        );
    }
}

fn location_root<'a>(
    template: &'a runinator_models::value::Value,
    base: &LocationBase,
) -> Option<&'a runinator_models::value::Value> {
    match base {
        LocationBase::Parameters => template.get("parameters"),
        LocationBase::Wait => template.get("wait"),
        LocationBase::Condition => template.get("condition"),
        LocationBase::Action => template.get("action"),
        LocationBase::Transitions => template.get("transitions"),
        LocationBase::TopLevel => Some(template),
    }
}

fn value_at_path<'a>(
    root: &'a runinator_models::value::Value,
    path: &[String],
) -> Option<&'a runinator_models::value::Value> {
    path.iter()
        .try_fold(root, |value, segment| value.get(segment.as_str()))
}

#[test]
fn node_kind_catalog_field_locations_exist_in_default_template() {
    for entry in node_kind_catalog() {
        for field in &entry.fields {
            let root = location_root(&entry.default_template, &field.location.base);
            let value = root.and_then(|root| value_at_path(root, &field.location.path));
            if value.is_some() {
                continue;
            }

            let parent = field
                .location
                .path
                .split_last()
                .and_then(|(_, path)| root.and_then(|root| value_at_path(root, path)));
            assert!(
                !field.field.param.required && parent.is_some_and(|value| value.is_object()),
                "{:?} field '{}' location {:?} is absent from its default template",
                entry.kind,
                field.field.param.name,
                field.location
            );
        }
    }
}

#[test]
fn node_kind_catalog_edge_slot_targets_exist_in_default_template() {
    for entry in node_kind_catalog() {
        for edge_slot in &entry.edge_slots {
            let value = location_root(&entry.default_template, &edge_slot.target.base)
                .and_then(|root| value_at_path(root, &edge_slot.target.path));
            assert!(
                value.is_some(),
                "{:?} edge slot '{}' target {:?} is absent from its default template",
                entry.kind,
                edge_slot.key,
                edge_slot.target
            );
        }
    }
}

#[test]
fn trigger_catalog_default_configuration_round_trips() {
    let catalog = trigger_kind_catalog();
    let cron = catalog
        .iter()
        .find(|item| item.kind == WorkflowTriggerKind::Cron)
        .unwrap();
    let manual = catalog
        .iter()
        .find(|item| item.kind == WorkflowTriggerKind::Manual)
        .unwrap();
    let chained = catalog
        .iter()
        .find(|item| item.kind == WorkflowTriggerKind::Chained)
        .unwrap();

    assert!(cron.default_configuration.get("cron").is_some());
    assert_eq!(
        chained.default_configuration.get("on"),
        Some(&runinator_models::value::Value::from("success"))
    );
    assert!(
        chained
            .default_configuration
            .get("target_workflow")
            .is_some()
    );
    serde_json::to_string(&chained.default_configuration)
        .expect("chained configuration serializes");
    assert_eq!(
        manual.default_configuration,
        runinator_models::value::Value::Object(Default::default())
    );
    serde_json::to_string(&cron.default_configuration).expect("cron configuration serializes");
    serde_json::to_string(&manual.default_configuration).expect("manual configuration serializes");
}

#[test]
fn catalog_metadata_serializes_to_stable_json_shape() {
    let node_catalog = serde_json::to_value(node_kind_catalog()).expect("node catalog serializes");
    serde_json::to_value(trigger_kind_catalog()).expect("trigger catalog serializes");
    serde_json::to_value(enum_catalogs()).expect("enum catalog serializes");

    let node = node_catalog
        .as_array()
        .unwrap()
        .iter()
        .find(|entry| entry["kind"] == "action")
        .unwrap();
    assert!(node.get("edge_slots").is_some());
    assert!(node.get("default_template").is_some());
    assert!(node.get("supports_predicate_edges").is_some());
}
