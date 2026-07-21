use runinator_models::pipelines::{PipelineFailurePolicy, PipelineLinkSelector};

use super::{parse_pipeline_str, pipeline_to_wdlp};

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
fn round_trips_through_wdlp_render() {
    let bundle = parse_pipeline_str(SDLC).expect("parse");
    let rendered = pipeline_to_wdlp(&bundle);
    let reparsed = parse_pipeline_str(&rendered).expect("reparse");
    assert_eq!(bundle, reparsed);
}
