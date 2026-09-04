//! Portable workspace authoring survives source and module round trips.
use runinator_models::{
    json,
    workflow_vm::{WorkflowEffectRequest, WorkflowInstruction},
};
use runinator_rexrap::{
    CompileOptions, compile_str, decompile, format_str, parse_pipeline_str, pipeline_to_rexrapp,
};

#[test]
fn workflow_default_survives_round_trip_and_compiles_completion_checkpoint() {
    let source = r#"namespace acme.tests
workflow "Portable" v1 {
    key portable
    workspace { key: "report", create: true }
    do { let work = console.run(command: "echo hello") }
}"#;
    let options = CompileOptions {
        enabled: true,
        ..Default::default()
    };
    let definition = compile_str(source, &options).unwrap();
    assert_eq!(
        definition.definition.metadata.get("workspace"),
        Some(&json!({"key":"report","create":true}))
    );
    for source in [format_str(source).unwrap(), decompile(&definition).unwrap()] {
        let reparsed = compile_str(&source, &options).unwrap();
        assert_eq!(
            reparsed.definition.metadata.get("workspace"),
            definition.definition.metadata.get("workspace")
        );
    }
    let module = runinator_workflows::compile_workflow_module(&definition).unwrap();
    assert!(module.instructions.iter().any(|instruction| matches!(instruction, WorkflowInstruction::Effect { request: WorkflowEffectRequest::Action { provider, function, .. } } if provider == "workspace" && function == "checkpoint")));
}

#[test]
fn pipeline_defaults_and_member_overrides_round_trip() {
    let source = r#"pipeline "Portable" {
        workspace { key: "report", create: true }
        workflow "acme.test.first"
        workflow "acme.test.second" with_workspace params.workspace
        "acme.test.first" -> "acme.test.second" on success with { workspace: source.outputs.workspaces["report"] }
    }"#;
    let pipeline = parse_pipeline_str(source).unwrap();
    assert!(pipeline.pipelines[0].defaults.workspace.is_some());
    assert!(pipeline.pipelines[0].members[1].workspace.is_some());
    assert_eq!(
        parse_pipeline_str(&pipeline_to_rexrapp(&pipeline)).unwrap(),
        pipeline
    );
}
