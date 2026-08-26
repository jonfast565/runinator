//! covers manifest validation, the exclude matcher, and the property the whole archive format
//! exists for: the same tree always produces the same digest.

use super::*;

use std::fs;
use std::path::PathBuf;

fn scratch(name: &str) -> PathBuf {
    let directory = std::env::temp_dir().join(format!("runi-fn-{name}-{}", std::process::id()));
    let _ = fs::remove_dir_all(&directory);
    fs::create_dir_all(&directory).unwrap();
    directory
}

fn write(root: &Path, relative: &str, contents: &str) {
    let path = root.join(relative);
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).unwrap();
    }
    fs::write(path, contents).unwrap();
}

const MANIFEST: &str = r#"{
  "name": "image-tools",
  "namespace": "runinator.examples",
  "description": "image utilities",
  "runtime": { "runtime": "python3.13" },
  "exports": [
    {
      "name": "resize",
      "handler": "src.images.resize",
      "input": [{ "name": "source", "type": "string", "required": true }],
      "output": [{ "name": "uri", "type": "string" }]
    }
  ]
}"#;

fn package(name: &str) -> PathBuf {
    let root = scratch(name);
    write(&root, MANIFEST_FILE, MANIFEST);
    write(
        &root,
        "src/images.py",
        "def resize(source):\n    return source\n",
    );
    write(&root, "README.md", "# image tools\n");
    root
}

#[test]
fn reads_a_manifest_into_a_publish_request() {
    let root = package("manifest");
    let source = FunctionSource::load(&root).unwrap();

    assert_eq!(source.qualified_name(), "runinator.examples.image-tools");
    let request = source.publish_request();
    assert_eq!(request.package.name, "image-tools");
    assert_eq!(request.exports.len(), 1);
    assert_eq!(request.exports[0].handler, "src.images.resize");
    // the default alias moves unless the manifest opts out, so a plain publish is immediately live.
    assert_eq!(request.alias.as_deref(), Some("latest"));
    // limits are absent from the manifest but must still arrive bounded.
    assert!(request.exports[0].limits.timeout_seconds > 0);
    assert!(!request.exports[0].limits.network);
    // the manifest is kept verbatim so a later publish can be diffed against what was published.
    assert!(request.manifest.get("exports").is_some());

    fs::remove_dir_all(&root).unwrap();
}

#[test]
fn provisional_catalog_entries_bind_same_pack_calls_deterministically() {
    let root = package("provisional-catalog");
    let first = FunctionSource::load(&root).unwrap();
    let second = FunctionSource::load(&root).unwrap();

    let entries = first.provisional_catalog_entries();
    assert_eq!(entries.len(), 1);
    let entry = &entries[0];
    assert_eq!(
        entry.provider_name(),
        "functions.runinator.examples.image-tools"
    );
    assert_eq!(entry.export_name, "resize");
    assert_eq!(
        entry.version,
        runinator_models::functions::PROVISIONAL_FUNCTION_VERSION
    );
    assert!(entry.binding().is_provisional());
    // Re-reading an unchanged package produces exactly the same temporary identity, so source
    // diagnostics and compiled zip contents do not depend on the machine applying the pack.
    assert_eq!(entries, second.provisional_catalog_entries());

    fs::remove_dir_all(&root).unwrap();
}

#[test]
fn the_same_tree_always_produces_the_same_digest() {
    let first = package("digest-a");
    let second = package("digest-b");

    // written second, and in a different directory, so mtimes and paths both differ.
    let left = archive_directory(&first, &[]).unwrap();
    let right = archive_directory(&second, &[]).unwrap();

    assert_eq!(left.digest, right.digest);
    assert_eq!(left.bytes, right.bytes);
    assert!(left.digest.starts_with("sha256:"));

    // and a changed byte must change it, or content addressing is not addressing content.
    write(
        &second,
        "src/images.py",
        "def resize(source):\n    return None\n",
    );
    let changed = archive_directory(&second, &[]).unwrap();
    assert_ne!(left.digest, changed.digest);

    fs::remove_dir_all(&first).unwrap();
    fs::remove_dir_all(&second).unwrap();
}

#[test]
fn archives_a_stable_sorted_file_list() {
    let root = package("contents");
    write(&root, "zzz.py", "");
    write(&root, "aaa.py", "");

    let archive = archive_directory(&root, &[]).unwrap();
    let mut sorted = archive.files.clone();
    sorted.sort();
    assert_eq!(archive.files, sorted);
    assert!(archive.files.contains(&"src/images.py".to_string()));
    // the manifest ships inside the archive so the package is self-describing at runtime.
    assert!(archive.files.contains(&MANIFEST_FILE.to_string()));

    fs::remove_dir_all(&root).unwrap();
}

#[test]
fn skips_build_output_and_honours_extra_excludes() {
    let root = package("excludes");
    write(&root, "target/debug/artifact", "junk");
    write(&root, "src/__pycache__/images.pyc", "junk");
    write(&root, ".git/config", "junk");
    write(&root, "notes.txt", "junk");

    let archive = archive_directory(&root, &["notes.txt".to_string()]).unwrap();
    assert!(!archive.files.iter().any(|file| file.starts_with("target/")));
    assert!(
        !archive
            .files
            .iter()
            .any(|file| file.contains("__pycache__"))
    );
    assert!(!archive.files.iter().any(|file| file.starts_with(".git/")));
    assert!(!archive.files.contains(&"notes.txt".to_string()));
    assert!(archive.files.contains(&"README.md".to_string()));

    fs::remove_dir_all(&root).unwrap();
}

#[test]
fn matches_globs_across_and_within_segments() {
    assert!(super::glob::matches("**/*.pyc", "src/a/b.pyc"));
    assert!(super::glob::matches("**/*.pyc", "b.pyc"));
    assert!(!super::glob::matches("**/*.pyc", "src/b.py"));
    assert!(super::glob::matches("target/", "target/debug/x"));
    assert!(!super::glob::matches("target/", "targets/debug/x"));
    // a single `*` must not cross a segment boundary.
    assert!(!super::glob::matches("src/*", "src/a/b"));
    assert!(super::glob::matches("src/*", "src/a"));
    assert!(super::glob::matches("src/?.py", "src/a.py"));
}

#[test]
fn rejects_a_manifest_that_could_not_be_called() {
    let no_exports = r#"{"name":"a","runtime":{"runtime":"python3.13"},"exports":[]}"#;
    let manifest: FunctionManifest = serde_json::from_str(no_exports).unwrap();
    assert!(manifest.validate().is_err());

    // a dot in a name would split the dotted call path in the wrong place.
    let dotted = r#"{"name":"a.b","runtime":{"runtime":"python3.13"},
        "exports":[{"name":"go","handler":"h"}]}"#;
    let manifest: FunctionManifest = serde_json::from_str(dotted).unwrap();
    assert!(manifest.validate().is_err());

    let duplicate = r#"{"name":"a","runtime":{"runtime":"python3.13"},
        "exports":[{"name":"go","handler":"h"},{"name":"go","handler":"i"}]}"#;
    let manifest: FunctionManifest = serde_json::from_str(duplicate).unwrap();
    assert!(manifest.validate().is_err());

    let nested_namespace = r#"{"name":"a","namespace":"acme.shared.tools","runtime":{"runtime":"python3.13"},
        "exports":[{"name":"go","handler":"h"}]}"#;
    let manifest: FunctionManifest = serde_json::from_str(nested_namespace).unwrap();
    assert!(manifest.validate().is_ok());

    let empty_namespace_segment = r#"{"name":"a","namespace":"acme..tools","runtime":{"runtime":"python3.13"},
        "exports":[{"name":"go","handler":"h"}]}"#;
    let manifest: FunctionManifest = serde_json::from_str(empty_namespace_segment).unwrap();
    assert!(manifest.validate().is_err());
}
