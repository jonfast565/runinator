use std::path::{Path, PathBuf};

use super::{
    load_pack_settings, load_workflow_bundle, pack_source_files, rexrap_context_workflow_signatures,
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
fn loads_hello_world_unified_source() {
    let manifest = repo_root()
        .join("packs")
        .join("hello-world")
        .join("hello-world.rrx");

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
fn rejects_headerless_pack_sources() {
    use std::fs;

    let path = std::env::temp_dir().join(format!(
        "runinator_headerless_pack_{}.rrx",
        std::process::id()
    ));
    fs::write(
        &path,
        "workflow \"Legacy\" v1 {\n\n    do {\n      let go = console.run(command: \"hi\")\n    }\n}\n",
    )
    .expect("write legacy pack");

    let error = load_workflow_bundle(&path).expect_err("headerless packs must be rejected");
    assert!(error.to_string().contains("language rexrap-1"));

    let _ = fs::remove_file(path);
}

#[test]
fn checked_in_packs_all_compile_and_settings_parse() {
    let packs_dir = repo_root().join("packs");
    let manifests = vec![packs_dir.join("hello-world"), packs_dir.join("creds-sync")];

    assert!(
        !manifests.is_empty(),
        "expected checked-in unified packs under {}",
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
    collect_files_with_extension(&packs_dir, "rrx", &mut rexrap_files);
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
}

#[test]
fn directory_pack_loads_rexraps_settings() {
    use std::fs;

    let dir = std::env::temp_dir().join(format!("runinator_rexraps_pack_{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).expect("temp pack dir");
    fs::write(
        dir.join("flow.rrx"),
        "language rexrap-1\n\nnamespace runinator.test {\nworkflow \"Temp\" v1 {\n    key temp\n\n    do {\n      let go = console.run(command: \"hi\")\n    }\n}\n}\n\nsettings {\nsecret app.token = \"abc\"\nconfig app.url = \"https://example.test\"\n}\n",
    )
    .expect("write rexrap");

    let bundle = load_workflow_bundle(&dir).expect("directory pack should load");
    assert_eq!(bundle.workflows.len(), 1);

    let settings = load_pack_settings(&dir)
        .expect("settings should load")
        .expect("settings block should be picked up");
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
        dir.join("child.rrx"),
        r#"language rexrap-1

namespace runinator.test {
workflow "Child" v1 returns { url: string } {
  params { id: string }
  key child

  do {
    console.run(command: params.id)
  }
}
}
"#,
    )
    .expect("write child");
    fs::write(
        dir.join("parent.rrx"),
        r#"language rexrap-1

namespace runinator.test {
workflow "Parent" v1 {
    key parent
    import workflow runinator.test.child as child

    do {
      let child_run = subflow("child", params: { id: "RUNI-1" })
      console.run(command: child_run.state.url)
    }
}
}
"#,
    )
    .expect("write parent");

    let bundle = load_workflow_bundle(&dir).expect("directory pack should type subflow");
    assert_eq!(bundle.workflows.len(), 2);

    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn directory_pack_embeds_imported_source_module_and_digest() {
    use std::fs;

    let dir = std::env::temp_dir().join(format!(
        "runinator_source_module_pack_{}",
        std::process::id()
    ));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).expect("temp pack dir");
    fs::write(
        dir.join("money.rrx"),
        r#"language rexrap-1

module acme.shared.money {
  fn echo(value: string) -> string = value
}

module acme.shared.unused {
  fn hidden(value: string) -> string = value
}
"#,
    )
    .expect("write module");
    fs::write(
        dir.join("workflow.rrx"),
        r#"language rexrap-1

namespace acme.billing {
workflow "Invoice" v1 {
  params { command: string }
  key invoice
  import module acme.shared.money as money

  do {
    console.run(command: money.echo(params.command))
  }
}
}
"#,
    )
    .expect("write workflow");

    let bundle = load_workflow_bundle(&dir).expect("directory pack should compile source module");
    assert_eq!(
        bundle.workflows.len(),
        1,
        "modules are not runtime artifacts"
    );
    let definition = &bundle.workflows[0];
    let encoded = serde_json::to_value(definition).expect("serialize workflow");
    let modules = encoded["definition"]["metadata"]["rexrap"]["source_modules"]
        .as_array()
        .expect("source module metadata");
    assert_eq!(modules.len(), 1);
    assert_eq!(modules[0]["path"], "acme.shared.money");
    assert!(
        modules[0]["digest"]
            .as_str()
            .is_some_and(|digest| digest.starts_with("sha256:"))
    );
    assert!(
        encoded["definition"]
            .to_string()
            .contains("__module_4acme_6shared_5money__echo"),
        "the resolved pure function should be embedded in the consuming workflow"
    );
    assert!(
        !encoded["definition"]
            .to_string()
            .contains("__module_4acme_6shared_6unused__hidden"),
        "an unimported module must not be embedded"
    );

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
        dir.join("child.rrx"),
        r#"workflow "Child" v1 {
  params { id: string }

    do {
      console.run(command: params.id)
    }
}
"#,
    )
    .expect("write child");

    let parent_path = dir.join("parent.rrx");
    let parent = r#"workflow "Parent" v1 {

    do {
      let child = subflow("Child", params: { id: "RUNI-1" })
    }
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
        .join("hello-world.rrx");

    let settings = load_pack_settings(&manifest).expect("loader should not error");

    assert!(
        settings.is_none(),
        "a manifest without a settings entry should not seed settings"
    );
}
