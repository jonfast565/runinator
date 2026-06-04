use std::path::Path;

use super::{load_pack_settings, load_workflow_bundle};

fn repo_root() -> &'static Path {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("runinator-ctl should live under the workspace root")
}

#[test]
fn loads_hello_world_smoke_pack_manifest() {
    let repo_root = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("runinator-ctl should live under the workspace root");
    let manifest = repo_root
        .join("packs")
        .join("hello-world")
        .join("hello-world.wdlp");

    let bundle = load_workflow_bundle(&manifest).expect("hello-world pack should load");

    assert_eq!(bundle.workflows.len(), 1);
    assert_eq!(bundle.workflows[0].name, "Hello World Test");
    assert_eq!(bundle.workflows[0].version, 1);
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
