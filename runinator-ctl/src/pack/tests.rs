use std::path::Path;

use super::load_workflow_bundle;

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
