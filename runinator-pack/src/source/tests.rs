use std::path::{Path, PathBuf};

use super::{load_pack_settings, load_workflow_bundle, pack_source_files};

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
        .join("hello-world.wdlp");

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
    collect_files_with_extension(&packs_dir, "wdlp", &mut manifests);
    manifests.sort();

    assert!(
        !manifests.is_empty(),
        "expected checked-in .wdlp manifests under {}",
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

    let mut wdl_files = Vec::new();
    collect_files_with_extension(&packs_dir, "wdl", &mut wdl_files);
    wdl_files.sort();

    for wdl_path in wdl_files {
        if manifest_sources.contains(&wdl_path) {
            continue;
        }
        let bundle = load_workflow_bundle(&wdl_path).unwrap_or_else(|err| {
            panic!(
                "standalone WDL {} failed to compile: {err}",
                wdl_path.display()
            )
        });
        for workflow in &bundle.workflows {
            runinator_workflows::validate_workflow(workflow).unwrap_or_else(|err| {
                panic!(
                    "workflow '{}' from {} failed validation: {err}",
                    workflow.name,
                    wdl_path.display()
                )
            });
        }
    }

    let mut settings_files = Vec::new();
    collect_files_with_extension(&packs_dir, "wdls", &mut settings_files);
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
    let manifest = repo_root().join("packs").join("sdlc").join("sdlc.wdlp");

    let settings = load_pack_settings(&manifest)
        .expect("sdlc settings should load")
        .expect("sdlc manifest declares a settings file");

    assert!(
        !settings.secrets.is_empty(),
        "sdlc settings bundle should seed config/secret slots"
    );
}

#[test]
fn directory_pack_loads_wdls_settings() {
    use std::fs;

    let dir = std::env::temp_dir().join(format!("runinator_wdls_pack_{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).expect("temp pack dir");
    fs::write(
        dir.join("flow.wdl"),
        "workflow \"Temp\" v1 {\n  node go = console.run(command: \"hi\")\n}\n",
    )
    .expect("write wdl");
    fs::write(
        dir.join("settings.wdls"),
        "secret app.token = \"abc\"\nconfig app.url = \"https://example.test\"\n",
    )
    .expect("write wdls");

    let bundle = load_workflow_bundle(&dir).expect("directory pack should load");
    assert_eq!(bundle.workflows.len(), 1);

    let settings = load_pack_settings(&dir)
        .expect("settings should load")
        .expect("settings.wdls should be picked up");
    assert_eq!(settings.secrets.len(), 2);
    assert_eq!(settings.secrets[0].scope, "app");
    assert_eq!(settings.secrets[0].name, "token");

    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn manifest_without_settings_entry_yields_none() {
    let manifest = repo_root()
        .join("packs")
        .join("hello-world")
        .join("hello-world.wdlp");

    let settings = load_pack_settings(&manifest).expect("loader should not error");

    assert!(
        settings.is_none(),
        "a manifest without a settings entry should not seed settings"
    );
}
