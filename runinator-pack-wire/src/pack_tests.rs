use super::*;
use runinator_models::bundles::{SecretBundle, SecretBundleEntry};
use runinator_models::pipelines::{
    PipelineLinkSelector, PipelineLinkSpec, PipelineMemberSpec, PipelineSpec,
};
use runinator_models::settings::SettingKind;
use runinator_models::value::Value;
use runinator_models::workflows::{WorkflowBundle, WorkflowDefinition};
use std::io::Write;

#[test]
fn pack_zip_round_trips() {
    let workflows = WorkflowBundle {
        workflows: vec![WorkflowDefinition {
            id: None,
            name: "demo".into(),
            key: Some("demo".into()),
            namespace: Some("runinator.test".into()),
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
        settings: vec![SecretBundleEntry {
            scope: "jira".into(),
            name: "token".into(),
            value: Value::from("abc"),
            schema: None,
            kind: SettingKind::Secret,
            expires_at: None,
            updated_at: None,
        }],
        ..Default::default()
    };

    let pipelines = PipelineBundle {
        pipelines: vec![PipelineSpec {
            name: "Core SDLC".into(),
            key: Some("core".into()),
            namespace: Some("runinator.test".into()),
            description: Some("demo pipeline".into()),
            defaults: Default::default(),
            members: vec![PipelineMemberSpec {
                workspace: None,
                name: "demo".into(),
                failure_mode: None,
            }],
            links: vec![PipelineLinkSpec {
                from: "demo".into(),
                to: "demo".into(),
                on: PipelineLinkSelector::Complete,
                enabled: true,
                parameters: Default::default(),
            }],
            joins: vec![],
            concurrency: Default::default(),
            metadata: runinator_models::json!({}),
            triggers: vec![],
        }],
    };

    let zipped = build_pack_zip(&workflows, Some(&secrets), Some(&pipelines)).expect("zip");
    let mut archive = zip::ZipArchive::new(Cursor::new(&zipped)).expect("open zip");
    assert!(archive.by_name(SETTINGS_ENTRY).is_ok());
    assert!(archive.by_name(SECRETS_ENTRY).is_err());
    let contents = read_pack_zip(&zipped).expect("unzip");
    assert_eq!(contents.workflows.workflows.len(), 1);
    assert_eq!(contents.workflows.workflows[0].name, "demo");
    let read_secrets = contents.settings.expect("settings present");
    assert_eq!(read_secrets.settings, secrets.settings);
    assert_eq!(contents.pipelines.expect("pipelines present"), pipelines);

    // secrets and pipelines are optional.
    let contents =
        read_pack_zip(&build_pack_zip(&workflows, None, None).expect("zip")).expect("unzip");
    assert!(contents.settings.is_none());
    assert!(contents.pipelines.is_none());
}

#[test]
fn a_pack_carries_functions_and_their_artifacts() {
    use runinator_models::functions::{
        FunctionRuntimeSpec, NewFunctionExport, NewFunctionPackage, NewFunctionVersion,
    };

    let workflows = WorkflowBundle {
        workflows: vec![],
        triggers: vec![],
    };
    let digest = format!("sha256:{}", "a".repeat(64));
    let publish = NewFunctionVersion {
        package: NewFunctionPackage {
            name: "image-tools".into(),
            namespace: Some("runinator.test".into()),
            description: None,
            org_id: None,
        },
        artifact_digest: digest.clone(),
        manifest: Default::default(),
        runtime: FunctionRuntimeSpec::new("python3.13"),
        exports: vec![NewFunctionExport {
            name: "resize".into(),
            handler: "src.images.resize".into(),
            description: None,
            input: vec![],
            output: vec![],
            limits: Default::default(),
        }],
        alias: Some("latest".into()),
    };

    let zipped = PackBuilder::new(&workflows)
        .functions(vec![publish.clone()])
        .function_artifact(digest.clone(), b"archive bytes".to_vec())
        .build()
        .expect("zip");
    let contents = read_pack_zip(&zipped).expect("unzip");

    assert_eq!(contents.functions, vec![publish]);
    // the digest is recovered from the entry name, so the reader never has to trust a manifest to
    // tell it what bytes it is holding.
    assert_eq!(
        contents.function_artifacts.get(&digest).map(Vec::as_slice),
        Some(b"archive bytes".as_slice())
    );
}

#[test]
fn a_pack_without_functions_reads_back_empty_rather_than_failing() {
    let workflows = WorkflowBundle {
        workflows: vec![],
        triggers: vec![],
    };
    // every existing pack in the wild has no `functions.json`, so absence has to be ordinary.
    let contents =
        read_pack_zip(&build_pack_zip(&workflows, None, None).expect("zip")).expect("unzip");
    assert!(contents.functions.is_empty());
    assert!(contents.function_artifacts.is_empty());
}

#[test]
fn a_pack_may_carry_a_publish_without_its_artifact() {
    use runinator_models::functions::{
        FunctionRuntimeSpec, NewFunctionPackage, NewFunctionVersion,
    };

    let workflows = WorkflowBundle {
        workflows: vec![],
        triggers: vec![],
    };
    let publish = NewFunctionVersion {
        package: NewFunctionPackage {
            name: "image-tools".into(),
            namespace: Some("runinator.test".into()),
            description: None,
            org_id: None,
        },
        artifact_digest: format!("sha256:{}", "b".repeat(64)),
        manifest: Default::default(),
        runtime: FunctionRuntimeSpec::new("python3.13"),
        exports: vec![],
        alias: None,
    };

    // this is the normal case on a re-apply: the client asked first, the server already held the
    // bytes, so only the publish rides along. carrying every artifact every time would push
    // megabytes through the request limit to re-send what the server has.
    let zipped = PackBuilder::new(&workflows)
        .functions(vec![publish])
        .build()
        .expect("zip");
    let contents = read_pack_zip(&zipped).expect("unzip");
    assert_eq!(contents.functions.len(), 1);
    assert!(contents.function_artifacts.is_empty());
}

#[test]
fn old_unnamespaced_compiled_packs_are_rejected() {
    let workflows = WorkflowBundle {
        workflows: vec![WorkflowDefinition {
            id: None,
            name: "legacy".into(),
            key: None,
            namespace: None,
            org_id: None,
            version: Default::default(),
            enabled: true,
            input_type: Default::default(),
            definition: Default::default(),
            created_at: None,
            updated_at: None,
        }],
        triggers: Vec::new(),
    };

    let error = build_pack_zip(&workflows, None, None).expect_err("legacy pack must fail");
    assert!(error.to_string().contains("dotted namespace"));
}

fn raw_zip(entries: Vec<(String, Vec<u8>)>) -> Vec<u8> {
    let mut buffer = Vec::new();
    {
        let mut zip = zip::ZipWriter::new(Cursor::new(&mut buffer));
        let options = zip::write::SimpleFileOptions::default()
            .compression_method(zip::CompressionMethod::Stored);
        for (name, bytes) in entries {
            zip.start_file(name, options).unwrap();
            zip.write_all(&bytes).unwrap();
        }
        zip.finish().unwrap();
    }
    buffer
}

#[test]
fn rejects_too_many_archive_entries() {
    let entries = (0..=MAX_PACK_ENTRIES)
        .map(|index| (format!("unused/{index}"), Vec::new()))
        .collect();
    let zipped = raw_zip(entries);

    let error = read_pack_zip(&zipped).err().expect("entry flood must fail");
    assert!(error.to_string().contains("limit is"));
}

#[test]
fn rejects_an_oversized_uncompressed_entry() {
    let zipped = raw_zip(vec![(
        WORKFLOWS_ENTRY.into(),
        vec![b' '; MAX_PACK_ENTRY_BYTES as usize + 1],
    )]);

    let error = read_pack_zip(&zipped)
        .err()
        .expect("oversized entry must fail");
    assert!(error.to_string().contains("per-entry limit"));
}

#[test]
fn reads_legacy_secrets_json_as_version_one_settings() {
    let workflows = WorkflowBundle::default();
    let legacy = serde_json::json!({
        "secrets": [{
            "scope": "github",
            "name": "token",
            "value": "legacy",
            "kind": "secret"
        }]
    });
    let zipped = raw_zip(vec![
        (
            WORKFLOWS_ENTRY.into(),
            serde_json::to_vec(&workflows).unwrap(),
        ),
        (SECRETS_ENTRY.into(), serde_json::to_vec(&legacy).unwrap()),
    ]);

    let settings = read_pack_zip(&zipped)
        .expect("legacy pack")
        .settings
        .expect("legacy settings");
    assert_eq!(settings.version, 1);
    assert_eq!(settings.settings.len(), 1);
    assert!(settings.execution_profiles.is_empty());
}

#[test]
fn rejects_both_settings_entry_names() {
    let workflows = WorkflowBundle::default();
    let settings = SecretBundle::default();
    let zipped = raw_zip(vec![
        (
            WORKFLOWS_ENTRY.into(),
            serde_json::to_vec(&workflows).unwrap(),
        ),
        (
            SETTINGS_ENTRY.into(),
            serde_json::to_vec(&settings).unwrap(),
        ),
        (SECRETS_ENTRY.into(), serde_json::to_vec(&settings).unwrap()),
    ]);

    let error = read_pack_zip(&zipped)
        .err()
        .expect("ambiguous pack must fail");
    assert!(error.to_string().contains("cannot contain both"));
}
