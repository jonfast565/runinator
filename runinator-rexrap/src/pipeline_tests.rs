use runinator_models::pipelines::{
    PipelineFailurePolicy, PipelineJoinMode, PipelineLinkSelector, PipelineMemberFailureMode,
};
use runinator_models::schedules::ConcurrencyPolicy;

use super::{parse_pipeline_str, pipeline_to_rexrapp};

const SDLC: &str = r#"
pipeline "Core SDLC" {
    description "Status-driven SDLC scanner pipeline."
    on_failure continue
    max_depth 8

    workflow "SDLC: Development"
    workflow "SDLC: Review"
    workflow "SDLC: Deploy"
    workflow "SDLC: QA"

    "SDLC: Development" -> "SDLC: Review" on complete
    "SDLC: Review"      -> "SDLC: Deploy" on complete
    "SDLC: Deploy"      -> "SDLC: QA"     on complete
}
"#;

#[test]
fn parses_pipeline_members_links_and_defaults() {
    let bundle = parse_pipeline_str(SDLC).expect("parse");
    assert_eq!(bundle.pipelines.len(), 1);
    let p = &bundle.pipelines[0];
    assert_eq!(p.name, "Core SDLC");
    assert_eq!(
        p.description.as_deref(),
        Some("Status-driven SDLC scanner pipeline.")
    );
    assert_eq!(p.defaults.on_step_failure, PipelineFailurePolicy::Continue);
    assert_eq!(p.defaults.max_chain_depth, Some(8));
    assert_eq!(p.members.len(), 4);
    assert_eq!(p.links.len(), 3);
    assert_eq!(p.links[0].from, "SDLC: Development");
    assert_eq!(p.links[0].to, "SDLC: Review");
    assert_eq!(p.links[0].on, PipelineLinkSelector::Complete);
    assert!(p.links.iter().all(|l| l.enabled));
}

#[test]
fn pipeline_key_and_namespace_round_trip() {
    let source = r#"
pipeline "Release train" {
    key release_train
    namespace acme.delivery
    workflow "Build"
}
"#;
    let bundle = parse_pipeline_str(source).unwrap();
    let spec = &bundle.pipelines[0];
    assert_eq!(spec.key.as_deref(), Some("release_train"));
    assert_eq!(spec.namespace.as_deref(), Some("acme.delivery"));
    let rendered = pipeline_to_rexrapp(&bundle);
    let reparsed = parse_pipeline_str(&rendered).unwrap();
    assert_eq!(reparsed, bundle);
}

#[test]
fn link_selector_defaults_from_failure_policy() {
    // halt (default) -> links without `on` fire on success.
    let halt = parse_pipeline_str(r#"pipeline "P" { workflow "A" workflow "B" "A" -> "B" }"#)
        .expect("parse");
    assert_eq!(halt.pipelines[0].links[0].on, PipelineLinkSelector::Success);

    // continue -> links without `on` fire on complete.
    let cont = parse_pipeline_str(
        r#"pipeline "P" { on_failure continue workflow "A" workflow "B" "A" -> "B" }"#,
    )
    .expect("parse");
    assert_eq!(
        cont.pipelines[0].links[0].on,
        PipelineLinkSelector::Complete
    );
}

#[test]
fn rejects_link_to_undeclared_member() {
    let err = parse_pipeline_str(r#"pipeline "P" { workflow "A" "A" -> "Ghost" on success }"#)
        .unwrap_err();
    assert!(
        err.to_string()
            .to_lowercase()
            .contains("not a declared workflow member")
    );
}

#[test]
fn rejects_pipeline_without_members() {
    let err = parse_pipeline_str(r#"pipeline "Empty" { }"#).unwrap_err();
    assert!(err.to_string().to_lowercase().contains("at least one"));
}

#[test]
fn round_trips_through_rexrapp_render() {
    let bundle = parse_pipeline_str(SDLC).expect("parse");
    let rendered = pipeline_to_rexrapp(&bundle);
    let reparsed = parse_pipeline_str(&rendered).expect("reparse");
    assert_eq!(bundle, reparsed);
}

const TRIGGERED: &str = r#"
pipeline "Nightly" {
    trigger cron "0 0 * * *"
    trigger on_success workflow "Upstream"
    trigger on_complete pipeline "Other" disabled

    workflow "A"
    workflow "B"

    "A" -> "B" on success
}
"#;

#[test]
fn parses_pipeline_triggers() {
    use runinator_models::workflows::WorkflowTriggerKind;
    let bundle = parse_pipeline_str(TRIGGERED).expect("parse");
    let p = &bundle.pipelines[0];
    assert_eq!(p.triggers.len(), 3);

    let cron = &p.triggers[0];
    assert_eq!(cron.kind, WorkflowTriggerKind::Cron);
    assert!(cron.enabled);
    assert_eq!(
        cron.configuration.get("cron").and_then(|v| v.as_str()),
        Some("0 0 * * *")
    );

    let from_workflow = &p.triggers[1];
    assert_eq!(from_workflow.kind, WorkflowTriggerKind::Chained);
    assert_eq!(
        from_workflow
            .configuration
            .get("on")
            .and_then(|v| v.as_str()),
        Some("success")
    );
    assert_eq!(
        from_workflow
            .configuration
            .get("source_workflow")
            .and_then(|v| v.as_str()),
        Some("Upstream")
    );

    let from_pipeline = &p.triggers[2];
    assert!(!from_pipeline.enabled);
    assert_eq!(
        from_pipeline
            .configuration
            .get("source_pipeline")
            .and_then(|v| v.as_str()),
        Some("Other")
    );
}

#[test]
fn round_trips_pipeline_triggers() {
    let bundle = parse_pipeline_str(TRIGGERED).expect("parse");
    let rendered = pipeline_to_rexrapp(&bundle);
    let reparsed = parse_pipeline_str(&rendered).expect("reparse");
    assert_eq!(bundle, reparsed);
}

const MEMBER_FAILURE_MODES: &str = r#"
pipeline "Deploy" {
    workflow "Build" on_failure stop
    workflow "Test" on_failure silently_continue
    workflow "Notify" on_failure inquire
    workflow "Cleanup"

    "Build" -> "Test" on complete
    "Test" -> "Notify" on complete
    "Notify" -> "Cleanup" on complete
}
"#;

#[test]
fn parses_member_failure_modes() {
    let bundle = parse_pipeline_str(MEMBER_FAILURE_MODES).expect("parse");
    let p = &bundle.pipelines[0];
    assert_eq!(p.members[0].name, "Build");
    assert_eq!(
        p.members[0].failure_mode,
        Some(PipelineMemberFailureMode::Stop)
    );
    assert_eq!(
        p.members[1].failure_mode,
        Some(PipelineMemberFailureMode::SilentlyContinue)
    );
    assert_eq!(
        p.members[2].failure_mode,
        Some(PipelineMemberFailureMode::Inquire)
    );
    // a member with no `on_failure` clause takes the pipeline default at import time.
    assert_eq!(p.members[3].failure_mode, None);
}

#[test]
fn round_trips_member_failure_modes() {
    let bundle = parse_pipeline_str(MEMBER_FAILURE_MODES).expect("parse");
    let rendered = pipeline_to_rexrapp(&bundle);
    let reparsed = parse_pipeline_str(&rendered).expect("reparse");
    assert_eq!(bundle, reparsed);
}

const MAPPED_JOIN: &str = r#"
pipeline "Release" {
    concurrency 2 on_conflict queue

    workflow "Linux Build"
    workflow "macOS Build"
    workflow "Publish"

    "Linux Build" -> "Publish" on success
    "macOS Build" -> "Publish" on success
    join "Publish" all with {
        linux: members["Linux Build"].result,
        macos: members["macOS Build"].result,
        environment: params.environment
    }
}
"#;

#[test]
fn parses_and_round_trips_mappings_joins_and_concurrency() {
    let bundle = parse_pipeline_str(MAPPED_JOIN).expect("parse");
    let pipeline = &bundle.pipelines[0];
    assert_eq!(pipeline.concurrency.max_concurrent_runs, 2);
    assert_eq!(pipeline.concurrency.on_conflict, ConcurrencyPolicy::Queue);
    assert_eq!(pipeline.joins.len(), 1);
    assert_eq!(pipeline.joins[0].mode, PipelineJoinMode::All);
    assert!(pipeline.joins[0].parameters.to_string().contains("members"));
    let rendered = pipeline_to_rexrapp(&bundle);
    assert_eq!(bundle, parse_pipeline_str(&rendered).expect("reparse"));
}

#[test]
fn rejects_ambiguous_multi_inbound_member_without_join() {
    let source = r#"pipeline "P" {
        workflow "A" workflow "B" workflow "C"
        "A" -> "C" on success
        "B" -> "C" on success
    }"#;
    assert!(
        parse_pipeline_str(source)
            .unwrap_err()
            .to_string()
            .contains("explicit join")
    );
}

#[test]
fn rejects_cycles_and_unsupported_mapping_roots() {
    let cycle = r#"pipeline "P" { workflow "A" workflow "B" "A" -> "B" "B" -> "A" }"#;
    assert!(
        parse_pipeline_str(cycle)
            .unwrap_err()
            .to_string()
            .contains("acyclic")
    );

    let bad_root = r#"pipeline "P" {
        workflow "A" workflow "B"
        "A" -> "B" with { value: unknown.result }
    }"#;
    assert!(
        parse_pipeline_str(bad_root)
            .unwrap_err()
            .to_string()
            .contains("unsupported")
    );
}

#[test]
fn rejects_invalid_first_success_join_selectors() {
    let source = r#"pipeline "P" {
        workflow "A" workflow "B" workflow "C"
        "A" -> "C" on complete
        "B" -> "C" on success
        join "C" first_success
    }"#;
    assert!(
        parse_pipeline_str(source)
            .unwrap_err()
            .to_string()
            .contains("success-selecting")
    );
}

#[test]
fn rejects_effectful_pipeline_mapping_calls() {
    let source = r#"pipeline "P" {
        workflow "A" workflow "B"
        "A" -> "B" with { generated_at: now() }
    }"#;
    let message = parse_pipeline_str(source).unwrap_err().to_string();
    assert!(message.contains("pure intrinsic"), "{message}");
}
