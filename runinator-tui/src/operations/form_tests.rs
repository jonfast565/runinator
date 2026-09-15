//! Type-guided launch form behavior.

use super::*;
use runinator_models::{
    types::RuninatorField,
    workflows::{WorkflowDefinition, WorkflowGraph},
};

#[test]
fn nested_fields_are_collected_and_validated() {
    let workflow = WorkflowDefinition {
        id: Some(Uuid::now_v7()),
        name: "guided".into(),
        key: None,
        namespace: None,
        org_id: None,
        version: Default::default(),
        enabled: true,
        input_type: RuninatorType::typed_structure([(
            "config",
            RuninatorField::required(RuninatorType::typed_structure([
                ("enabled", RuninatorField::required(RuninatorType::Boolean)),
                (
                    "names",
                    RuninatorField::required(RuninatorType::array(RuninatorType::String)),
                ),
            ])),
        )]),
        output_type: RuninatorType::Any,
        definition: WorkflowGraph::default(),
        created_at: None,
        updated_at: None,
    };
    let mut form = LaunchForm::new(&workflow).unwrap();
    assert!(!form.advance().unwrap());
    form.buffer = "true".into();
    assert!(!form.advance().unwrap());
    form.buffer = "[\"a\",\"b\"]".into();
    assert!(form.advance().unwrap());
    let (_, value, _) = form.finish().unwrap();
    assert_eq!(value["config"]["enabled"], Value::Bool(true));
}
