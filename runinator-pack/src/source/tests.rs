use std::path::Path;

use super::{load_pack_settings, load_workflow_bundle};

fn repo_root() -> &'static Path {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("runinator-pack should live under the workspace root")
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
        "workflow \"Temp\" v1 {\n  console.run(command: \"hi\")\n}\n",
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
