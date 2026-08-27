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

    workflow "acme.sdlc.development"
    workflow "acme.sdlc.review"
    workflow "acme.sdlc.deploy"
    workflow "acme.sdlc.qa"

    "acme.sdlc.development" -> "acme.sdlc.review" on complete
    "acme.sdlc.review"      -> "acme.sdlc.deploy" on complete
    "acme.sdlc.deploy"      -> "acme.sdlc.qa"     on complete
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
    assert_eq!(p.links[0].from, "acme.sdlc.development");
    assert_eq!(p.links[0].to, "acme.sdlc.review");
    assert_eq!(p.links[0].on, PipelineLinkSelector::Complete);
    assert!(p.links.iter().all(|l| l.enabled));
}

#[test]
fn pipeline_key_and_namespace_round_trip() {
    let source = r#"
pipeline "Release train" {
    key release_train
    namespace acme.delivery
    workflow "acme.delivery.build"
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
    let halt = parse_pipeline_str(r#"pipeline "P" { workflow "acme.test.a" workflow "acme.test.b" "acme.test.a" -> "acme.test.b" }"#)
        .expect("parse");
    assert_eq!(halt.pipelines[0].links[0].on, PipelineLinkSelector::Success);

    // continue -> links without `on` fire on complete.
    let cont = parse_pipeline_str(
        r#"pipeline "P" { on_failure continue workflow "acme.test.a" workflow "acme.test.b" "acme.test.a" -> "acme.test.b" }"#,
    )
    .expect("parse");
    assert_eq!(
        cont.pipelines[0].links[0].on,
        PipelineLinkSelector::Complete
    );
}

#[test]
fn rejects_link_to_undeclared_member() {
    let err = parse_pipeline_str(
        r#"pipeline "P" { workflow "acme.test.a" "acme.test.a" -> "acme.test.ghost" on success }"#,
    )
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
fn rejects_bare_pipeline_member_and_trigger_sources() {
    let member = parse_pipeline_str(r#"pipeline "P" { workflow "bare" }"#).unwrap_err();
    assert!(member.to_string().contains("canonical"), "{member}");

    let trigger = parse_pipeline_str(
        r#"pipeline "P" { trigger on_success workflow "bare" workflow "acme.test.member" }"#,
    )
    .unwrap_err();
    assert!(trigger.to_string().contains("canonical"), "{trigger}");
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
    trigger on_success workflow "acme.upstream.workflow"
    trigger on_complete pipeline "acme.other.pipeline" disabled

    workflow "acme.nightly.a"
    workflow "acme.nightly.b"

    "acme.nightly.a" -> "acme.nightly.b" on success
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
        Some("acme.upstream.workflow")
    );

    let from_pipeline = &p.triggers[2];
    assert!(!from_pipeline.enabled);
    assert_eq!(
        from_pipeline
            .configuration
            .get("source_pipeline")
            .and_then(|v| v.as_str()),
        Some("acme.other.pipeline")
    );
}

#[test]
fn round_trips_pipeline_triggers() {
    let bundle = parse_pipeline_str(TRIGGERED).expect("parse");
    let rendered = pipeline_to_rexrapp(&bundle);
    let reparsed = parse_pipeline_str(&rendered).expect("reparse");
    assert_eq!(bundle, reparsed);
}

#[test]
fn pipeline_ingress_policy_round_trips() {
    let source = r#"
pipeline "Release" {
    ingress scope "release.lifecycle" {
        on "created" when unbound -> start
        on "changed" when active -> queue
        on "reopened" when terminal -> requeue
    }

    workflow "acme.release.build"
}
"#;
    let bundle = parse_pipeline_str(source).expect("parse");
    let ingress = bundle.pipelines[0]
        .metadata
        .get("ingress")
        .expect("ingress metadata");
    assert_eq!(
        ingress.get("scope").and_then(|value| value.as_str()),
        Some("release.lifecycle")
    );
    assert_eq!(
        ingress
            .get("routes")
            .and_then(|value| value.as_array())
            .map(Vec::len),
        Some(3)
    );
    assert_eq!(
        bundle,
        parse_pipeline_str(&pipeline_to_rexrapp(&bundle)).expect("reparse")
    );
}

#[test]
fn generic_orchestration_policy_and_dispatch_predicates_round_trip() {
    let source = r#"
pipeline "Correlated work" {
    ingress scope "work-items" {
        on "updated" when active
            if "/changes/labels/removed" contains "auto"
            if "/active" == true
            -> dispatch "stop"
    }

    orchestration {
        intent "stop" effect terminate priority 100
        intent "rework" effect supersede priority 80 coalesce 5m restart "acme.work.planning"
        intent "pause" effect suspend priority 60 stop cancel restart current
        intent "continue" effect resume priority 50
        intent "revision_observed" effect signal priority 10 revision "/subject_revision"
        budget "deterministic" attempts 2 exhausted pause
        budget "transient" attempts 3 exhausted pause
        phase "acme.work.implementation" {
            subject_revision from "/candidate_revision"
            resources from "/resources"
            evidence from "/evidence"
            failure_class from "/failure_class"
            workspace scope "source" reuse labels { "capability": "git" }
        }
    }

    workflow "acme.work.planning"
    workflow "acme.work.implementation"
    "acme.work.planning" -> "acme.work.implementation" on success
}
"#;
    let bundle = parse_pipeline_str(source).expect("parse orchestration policy");
    let policy = bundle.pipelines[0]
        .metadata
        .get("orchestration")
        .expect("orchestration metadata");
    assert_eq!(
        policy
            .pointer("/intents/stop/priority")
            .and_then(|value| value.as_i64()),
        Some(100)
    );
    assert_eq!(
        policy
            .pointer("/phases/acme.work.implementation/workspace/scope")
            .and_then(|value| value.as_str()),
        Some("source")
    );
    let rendered = pipeline_to_rexrapp(&bundle);
    let reparsed = parse_pipeline_str(&rendered).expect("reparse orchestration policy");
    assert_eq!(bundle, reparsed);
}

#[test]
fn orchestration_policy_rejects_duplicate_priorities_and_unknown_members() {
    let duplicate = parse_pipeline_str(
        r#"
pipeline "P" {
    orchestration {
        intent "a" effect observe priority 10
        intent "b" effect signal priority 10
    }
    workflow "acme.test.member"
}
"#,
    )
    .unwrap_err();
    assert!(duplicate.to_string().contains("priority"), "{duplicate}");

    let unknown = parse_pipeline_str(
        r#"
pipeline "P" {
    orchestration {
        intent "rework" effect supersede priority 80 restart "acme.test.unknown"
    }
    workflow "acme.test.member"
}
"#,
    )
    .unwrap_err();
    assert!(unknown.to_string().contains("does not exist"), "{unknown}");
}

const MEMBER_FAILURE_MODES: &str = r#"
pipeline "Deploy" {
    workflow "acme.deploy.build" on_failure stop
    workflow "acme.deploy.test" on_failure silently_continue
    workflow "acme.deploy.notify" on_failure inquire
    workflow "acme.deploy.cleanup"

    "acme.deploy.build" -> "acme.deploy.test" on complete
    "acme.deploy.test" -> "acme.deploy.notify" on complete
    "acme.deploy.notify" -> "acme.deploy.cleanup" on complete
}
"#;

#[test]
fn parses_member_failure_modes() {
    let bundle = parse_pipeline_str(MEMBER_FAILURE_MODES).expect("parse");
    let p = &bundle.pipelines[0];
    assert_eq!(p.members[0].name, "acme.deploy.build");
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

    workflow "acme.release.linux_build"
    workflow "acme.release.macos_build"
    workflow "acme.release.publish"

    "acme.release.linux_build" -> "acme.release.publish" on success
    "acme.release.macos_build" -> "acme.release.publish" on success
    join "acme.release.publish" all with {
        linux: members["acme.release.linux_build"].result,
        macos: members["acme.release.macos_build"].result,
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
        workflow "acme.test.a" workflow "acme.test.b" workflow "acme.test.c"
        "acme.test.a" -> "acme.test.c" on success
        "acme.test.b" -> "acme.test.c" on success
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
    let cycle = r#"pipeline "P" { workflow "acme.test.a" workflow "acme.test.b" "acme.test.a" -> "acme.test.b" "acme.test.b" -> "acme.test.a" }"#;
    assert!(
        parse_pipeline_str(cycle)
            .unwrap_err()
            .to_string()
            .contains("acyclic")
    );

    let bad_root = r#"pipeline "P" {
        workflow "acme.test.a" workflow "acme.test.b"
        "acme.test.a" -> "acme.test.b" with { value: unknown.result }
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
        workflow "acme.test.a" workflow "acme.test.b" workflow "acme.test.c"
        "acme.test.a" -> "acme.test.c" on complete
        "acme.test.b" -> "acme.test.c" on success
        join "acme.test.c" first_success
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
        workflow "acme.test.a" workflow "acme.test.b"
        "acme.test.a" -> "acme.test.b" with { generated_at: now() }
    }"#;
    let message = parse_pipeline_str(source).unwrap_err().to_string();
    assert!(message.contains("pure intrinsic"), "{message}");
}
