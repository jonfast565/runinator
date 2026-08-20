use std::path::{Path, PathBuf};

use super::{
    load_pack_pipelines, load_pack_settings, load_workflow_bundle, pack_source_files,
    rexrap_context_workflow_signatures,
};

fn repo_root() -> &'static Path {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("runinator-pack should live under the workspace root")
}

fn collect_files_with_extension(dir: &Path, extension: &str, out: &mut Vec<PathBuf>) {
    let entries = std::fs::read_dir(dir)
        .unwrap_or_else(|err| panic!("failed to read {}: {err}", dir.display()));
    for entry in entries {
        let path = entry
            .unwrap_or_else(|err| panic!("failed to read entry in {}: {err}", dir.display()))
            .path();
        if path.is_dir() {
            collect_files_with_extension(&path, extension, out);
            continue;
        }
        if path.extension().and_then(|ext| ext.to_str()) == Some(extension) {
            out.push(path);
        }
    }
}

#[test]
fn loads_hello_world_smoke_pack_manifest() {
    let manifest = repo_root()
        .join("packs")
        .join("hello-world")
        .join("hello-world.rexrapm");

    let bundle = load_workflow_bundle(&manifest).expect("hello-world pack should load");

    assert_eq!(bundle.workflows.len(), 1);
    assert_eq!(bundle.workflows[0].name, "Hello World Test");
    assert_eq!(
        bundle.workflows[0].version,
        runinator_models::semver::SemVer::new(1, 0, 0)
    );
    assert!(bundle.triggers.is_empty());
}

#[test]
fn checked_in_packs_all_compile_and_settings_parse() {
    let packs_dir = repo_root().join("packs");
    let mut manifests = Vec::new();
    collect_files_with_extension(&packs_dir, "rexrapm", &mut manifests);
    manifests.sort();

    assert!(
        !manifests.is_empty(),
        "expected checked-in .rexrapm manifests under {}",
        packs_dir.display()
    );

    let mut manifest_sources = Vec::new();
    for manifest in &manifests {
        let bundle = load_workflow_bundle(manifest).unwrap_or_else(|err| {
            panic!(
                "pack manifest {} failed to compile: {err}",
                manifest.display()
            )
        });
        for workflow in &bundle.workflows {
            runinator_workflows::validate_workflow(workflow).unwrap_or_else(|err| {
                panic!(
                    "workflow '{}' from {} failed validation: {err}",
                    workflow.name,
                    manifest.display()
                )
            });
        }
        load_pack_settings(manifest).unwrap_or_else(|err| {
            panic!(
                "pack settings for {} failed to parse: {err}",
                manifest.display()
            )
        });
        manifest_sources.extend(pack_source_files(manifest).unwrap_or_else(|err| {
            panic!(
                "pack manifest {} failed source discovery: {err}",
                manifest.display()
            )
        }));
    }
    manifest_sources.sort();
    manifest_sources.dedup();

    let mut rexrap_files = Vec::new();
    collect_files_with_extension(&packs_dir, "rexrap", &mut rexrap_files);
    rexrap_files.sort();

    for rexrap_path in rexrap_files {
        if manifest_sources.contains(&rexrap_path) {
            continue;
        }
        let bundle = load_workflow_bundle(&rexrap_path).unwrap_or_else(|err| {
            panic!(
                "standalone REXRAP {} failed to compile: {err}",
                rexrap_path.display()
            )
        });
        for workflow in &bundle.workflows {
            runinator_workflows::validate_workflow(workflow).unwrap_or_else(|err| {
                panic!(
                    "workflow '{}' from {} failed validation: {err}",
                    workflow.name,
                    rexrap_path.display()
                )
            });
        }
    }

    let mut settings_files = Vec::new();
    collect_files_with_extension(&packs_dir, "rexraps", &mut settings_files);
    settings_files.sort();

    for settings_path in settings_files {
        if manifest_sources.contains(&settings_path) {
            continue;
        }
        super::parse_settings_file(&settings_path).unwrap_or_else(|err| {
            panic!(
                "settings file {} failed to parse: {err}",
                settings_path.display()
            )
        });
    }
}

#[test]
fn sdlc_manifest_settings_entry_loads_bundle() {
    let manifest = repo_root().join("packs").join("sdlc").join("sdlc.rexrapm");

    let settings = load_pack_settings(&manifest)
        .expect("sdlc settings should load")
        .expect("sdlc manifest declares a settings file");

    assert!(
        !settings.secrets.is_empty(),
        "sdlc settings bundle should seed config/secret slots"
    );
}

#[test]
fn sdlc_manifest_loads_core_pipeline() {
    let manifest = repo_root().join("packs").join("sdlc").join("sdlc.rexrapm");

    let pipelines = load_pack_pipelines(&manifest)
        .expect("sdlc pipelines should load")
        .expect("sdlc manifest declares a pipeline file");

    assert_eq!(pipelines.pipelines.len(), 1);
    let core = &pipelines.pipelines[0];
    assert_eq!(core.name, "Core SDLC");
    assert_eq!(core.members.len(), 4);
    assert_eq!(core.links.len(), 3);
    // every link's endpoints must be declared members (lowering enforces this).
    let member_names: Vec<&str> = core.members.iter().map(|m| m.name.as_str()).collect();
    for link in &core.links {
        assert!(member_names.contains(&link.from.as_str()));
        assert!(member_names.contains(&link.to.as_str()));
    }
}

#[test]
fn directory_pack_loads_rexraps_settings() {
    use std::fs;

    let dir = std::env::temp_dir().join(format!("runinator_rexraps_pack_{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).expect("temp pack dir");
    fs::write(
        dir.join("flow.rexrap"),
        "workflow \"Temp\" v1 {\n  node go <- console.run(command: \"hi\")\n}\n",
    )
    .expect("write rexrap");
    fs::write(
        dir.join("settings.rexraps"),
        "secret app.token = \"abc\"\nconfig app.url = \"https://example.test\"\n",
    )
    .expect("write rexraps");

    let bundle = load_workflow_bundle(&dir).expect("directory pack should load");
    assert_eq!(bundle.workflows.len(), 1);

    let settings = load_pack_settings(&dir)
        .expect("settings should load")
        .expect("settings.rexraps should be picked up");
    assert_eq!(settings.secrets.len(), 2);
    assert_eq!(settings.secrets[0].scope, "app");
    assert_eq!(settings.secrets[0].name, "token");

    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn directory_pack_types_pack_local_subflows() {
    use std::fs;

    let dir = std::env::temp_dir().join(format!(
        "runinator_typed_subflow_pack_{}",
        std::process::id()
    ));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).expect("temp pack dir");
    fs::write(
        dir.join("child.rexrap"),
        r#"workflow "Child" v1 returns { url: string } {
  params { id: string }
  console.run(command: params.id)
}
"#,
    )
    .expect("write child");
    fs::write(
        dir.join("parent.rexrap"),
        r#"workflow "Parent" v1 {
  node child <- subflow("Child", params: { id: "RUNI-1" })
  console.run(command: child.state.url)
}
"#,
    )
    .expect("write parent");

    let bundle = load_workflow_bundle(&dir).expect("directory pack should type subflow");
    assert_eq!(bundle.workflows.len(), 2);

    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn rexrap_context_signatures_include_sibling_workflows() {
    use std::fs;

    let dir = std::env::temp_dir().join(format!(
        "runinator_rexrap_context_signatures_{}",
        std::process::id()
    ));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).expect("temp pack dir");
    fs::write(
        dir.join("child.rexrap"),
        r#"workflow "Child" v1 {
  params { id: string }
  console.run(command: params.id)
}
"#,
    )
    .expect("write child");

    let parent_path = dir.join("parent.rexrap");
    let parent = r#"workflow "Parent" v1 {
  node child <- subflow("Child", params: { id: "RUNI-1" })
}
"#;
    let signatures =
        rexrap_context_workflow_signatures(&parent_path, Some(parent)).expect("context signatures");

    assert!(signatures.iter().any(|signature| signature.name == "Child"));
    assert!(
        signatures
            .iter()
            .any(|signature| signature.name == "Parent")
    );

    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn manifest_without_settings_entry_yields_none() {
    let manifest = repo_root()
        .join("packs")
        .join("hello-world")
        .join("hello-world.rexrapm");

    let settings = load_pack_settings(&manifest).expect("loader should not error");

    assert!(
        settings.is_none(),
        "a manifest without a settings entry should not seed settings"
    );
}
