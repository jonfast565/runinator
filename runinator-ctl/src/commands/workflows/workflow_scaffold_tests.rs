//! The offline scaffold creates a complete pack and never overwrites authored files.

use super::*;

fn scratch(label: &str) -> PathBuf {
    std::env::temp_dir().join(format!("runinator-scaffold-{label}-{}", Uuid::new_v4()))
}

#[test]
fn scaffold_creates_checkable_source_and_readme() {
    let directory = scratch("create");
    workflows_scaffold(&directory, Some("Deploy Service"), "acme.platform", false)
        .expect("scaffold");
    let source_path = fs::read_dir(&directory)
        .expect("generated directory")
        .filter_map(|entry| entry.ok().map(|entry| entry.path()))
        .find(|path| path.extension().and_then(|extension| extension.to_str()) == Some("rrx"))
        .expect("generated source path");
    let source = fs::read_to_string(source_path).expect("generated source");
    assert!(source.contains("namespace acme.platform"));
    assert!(source.contains("workflow \"Deploy Service\" v1"));
    assert!(source.contains("\"fixtures\""));
    assert!(directory.join("README.md").is_file());
    fs::remove_dir_all(directory).expect("cleanup");
}

#[test]
fn scaffold_refuses_a_non_empty_directory() {
    let directory = scratch("occupied");
    fs::create_dir_all(&directory).expect("directory");
    fs::write(directory.join("keep.txt"), "mine").expect("existing file");
    let error = workflows_scaffold(&directory, None, "local", false)
        .expect_err("non-empty directory must be refused");
    assert!(error.to_string().contains("non-empty"));
    assert_eq!(
        fs::read_to_string(directory.join("keep.txt")).expect("preserved file"),
        "mine"
    );
    fs::remove_dir_all(directory).expect("cleanup");
}
