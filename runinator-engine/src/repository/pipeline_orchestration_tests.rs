//! Pipeline orchestration graph behavior.

use super::*;

fn attempt(key: &str, result: Value) -> PipelineMemberAttempt {
    PipelineMemberAttempt {
        id: Uuid::now_v7(),
        pipeline_run_id: Uuid::now_v7(),
        member_key: key.into(),
        workflow_id: Uuid::now_v7(),
        attempt: 1,
        workflow_run_id: Some(Uuid::now_v7()),
        status: PipelineMemberAttemptStatus::Succeeded,
        parameters: Value::Null,
        result,
        message: None,
        created_at: Utc::now(),
        started_at: Some(Utc::now()),
        finished_at: Some(Utc::now()),
    }
}

fn member(key: &str) -> PipelineMember {
    PipelineMember {
        workspace: None,
        key: key.into(),
        workflow_id: Uuid::now_v7(),
        failure_mode: PipelineMemberFailureMode::Stop,
    }
}

fn branching_pipeline() -> Pipeline {
    Pipeline {
        id: Some(Uuid::now_v7()),
        name: "targeted epochs".into(),
        key: Some("targeted_epochs".into()),
        namespace: Some("runinator.tests".into()),
        description: None,
        org_id: None,
        enabled: true,
        graph: runinator_models::pipelines::PipelineGraph {
            version: runinator_models::pipelines::PIPELINE_GRAPH_VERSION,
            members: vec![member("A"), member("B"), member("C")],
            links: vec![PipelineLink {
                id: Uuid::now_v7(),
                from: "A".into(),
                to: "B".into(),
                on: PipelineLinkSelector::Complete,
                enabled: true,
                parameters: Value::Null,
            }],
            joins: Default::default(),
        },
        concurrency: Default::default(),
        defaults: Default::default(),
        metadata: Value::Null,
        created_at: None,
        updated_at: None,
    }
}

#[test]
fn targeted_pipeline_runs_only_wait_for_reachable_members() {
    let pipeline = branching_pipeline();

    assert_eq!(
        reachable_member_keys(&pipeline, Some("A")),
        HashSet::from(["A".to_string(), "B".to_string()])
    );
    assert_eq!(
        reachable_member_keys(&pipeline, Some("C")),
        HashSet::from(["C".to_string()])
    );
    assert_eq!(
        reachable_member_keys(&pipeline, None),
        HashSet::from(["A".to_string(), "B".to_string(), "C".to_string()])
    );
}

#[test]
fn pipeline_mapping_overlays_params_and_resolves_source_and_members() {
    let source = attempt(
        "Build",
        runinator_models::json!({ "result": { "artifact": "app.tgz" } }),
    );
    let linux = attempt(
        "Linux Build",
        runinator_models::json!({ "result": { "sha": "abc" } }),
    );
    let latest = HashMap::from([("Build", &source), ("Linux Build", &linux)]);
    let mapping = runinator_models::json!({
        "artifact": { "$ref": { "node": "source", "output": ["result", "artifact"] } },
        "linux": { "$ref": { "node": "members", "output": ["Linux Build", "result", "sha"] } },
        "environment": { "$ref": { "params": ["environment"] } }
    });
    let resolved = resolve_member_parameters(
        &runinator_models::json!({ "environment": "prod", "keep": true }),
        &mapping,
        Some(&source),
        &latest,
    )
    .expect("mapping");
    assert_eq!(
        resolved,
        runinator_models::json!({
            "environment": "prod", "keep": true, "artifact": "app.tgz", "linux": "abc"
        })
    );
}

#[test]
fn stop_failure_mode_suppresses_every_outbound_selector() {
    let link = PipelineLink {
        id: Uuid::now_v7(),
        from: "A".into(),
        to: "B".into(),
        on: PipelineLinkSelector::Complete,
        enabled: true,
        parameters: Value::Null,
    };
    assert!(!selector_matches(
        &link,
        PipelineMemberAttemptStatus::Failed,
        PipelineMemberFailureMode::Stop
    ));
    assert!(selector_matches(
        &link,
        PipelineMemberAttemptStatus::Failed,
        PipelineMemberFailureMode::Continue
    ));
}

#[test]
fn chained_pipeline_sources_match_the_resolved_uuid_not_the_diagnostic_path() {
    let source_id = Uuid::now_v7();
    let trigger = PipelineTrigger {
        id: Some(Uuid::now_v7()),
        pipeline_id: Uuid::now_v7(),
        kind: runinator_models::workflows::WorkflowTriggerKind::Chained,
        enabled: true,
        configuration: runinator_models::json!({
            // This path is intentionally stale after a source namespace move.
            "source_pipeline": "old.namespace.source",
            "source_pipeline_id": source_id.to_string(),
        }),
        next_execution: None,
        blackout_start: None,
        blackout_end: None,
        metadata: Value::Null,
        created_at: None,
        updated_at: None,
    };

    assert_eq!(
        trigger_source_id(&trigger, "source_pipeline_id"),
        Some(source_id)
    );
    assert_ne!(
        trigger_source_id(&trigger, "source_pipeline_id"),
        Some(Uuid::now_v7())
    );
}
