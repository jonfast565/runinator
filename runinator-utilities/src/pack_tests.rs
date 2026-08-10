use super::*;
use runinator_models::bundles::{SecretBundle, SecretBundleEntry};
use runinator_models::pipelines::{
    PipelineLinkSelector, PipelineLinkSpec, PipelineMemberSpec, PipelineSpec,
};
use runinator_models::settings::SettingKind;
use runinator_models::value::Value;
use runinator_models::workflows::{WorkflowBundle, WorkflowDefinition};

#[test]
fn pack_zip_round_trips() {
    let workflows = WorkflowBundle {
        workflows: vec![WorkflowDefinition {
            id: None,
            name: "demo".into(),
            namespace: None,
            org_id: None,
            version: runinator_models::semver::SemVer::new(1, 0, 0),
            enabled: true,
            input_type: Default::default(),
            definition: Default::default(),
            created_at: None,
            updated_at: None,
        }],
        triggers: Vec::new(),
    };
    let secrets = SecretBundle {
        secrets: vec![SecretBundleEntry {
            scope: "jira".into(),
            name: "token".into(),
            value: Value::from("abc"),
            schema: None,
            kind: SettingKind::Secret,
            updated_at: None,
        }],
    };

    let pipelines = PipelineBundle {
        pipelines: vec![PipelineSpec {
            name: "Core SDLC".into(),
            description: Some("demo pipeline".into()),
            defaults: Default::default(),
            members: vec![PipelineMemberSpec {
                name: "demo".into(),
                failure_mode: None,
            }],
            links: vec![PipelineLinkSpec {
                from: "demo".into(),
                to: "demo".into(),
                on: PipelineLinkSelector::Complete,
                enabled: true,
            }],
            triggers: vec![],
        }],
    };

    let zipped = build_pack_zip(&workflows, Some(&secrets), Some(&pipelines)).expect("zip");
    let contents = read_pack_zip(&zipped).expect("unzip");
    assert_eq!(contents.workflows.workflows.len(), 1);
    assert_eq!(contents.workflows.workflows[0].name, "demo");
    let read_secrets = contents.secrets.expect("secrets present");
    assert_eq!(read_secrets.secrets, secrets.secrets);
    assert_eq!(contents.pipelines.expect("pipelines present"), pipelines);

    // secrets and pipelines are optional.
    let contents =
        read_pack_zip(&build_pack_zip(&workflows, None, None).expect("zip")).expect("unzip");
    assert!(contents.secrets.is_none());
    assert!(contents.pipelines.is_none());
}
