//! Workspace archive round trips and hostile content rejection.
use super::*;

#[test]
fn files_results_and_permissions_round_trip() {
    let source = tempfile::tempdir().unwrap();
    fs::create_dir(source.path().join("nested")).unwrap();
    fs::write(source.path().join("nested/result.txt"), "saved output").unwrap();
    let results = BTreeMap::from([("answer".into(), Value::from(42))]);
    let packed = pack(source.path(), &results).unwrap();
    assert_eq!(packed.files[0].path, "nested/result.txt");
    let destination = tempfile::tempdir().unwrap();
    assert_eq!(
        unpack(&packed.bytes, destination.path(), &packed.sha256).unwrap(),
        results
    );
    assert_eq!(
        fs::read_to_string(destination.path().join("nested/result.txt")).unwrap(),
        "saved output"
    );
    assert!(unpack(&packed.bytes, destination.path(), &packed.sha256).is_err());
}

#[test]
fn rejects_checksum_mismatch_and_traversal() {
    let source = tempfile::tempdir().unwrap();
    let packed = pack(source.path(), &BTreeMap::new()).unwrap();
    assert!(unpack(&packed.bytes, source.path(), "bad checksum").is_err());
    for path in ["../outside", "/absolute", "a/../b", ""] {
        assert!(validate_path(Path::new(path)).is_err());
    }
}

#[cfg(unix)]
#[test]
fn safe_links_round_trip_and_external_links_fail() {
    let source = tempfile::tempdir().unwrap();
    fs::write(source.path().join("result"), "ok").unwrap();
    std::os::unix::fs::symlink("result", source.path().join("alias")).unwrap();
    let packed = pack(source.path(), &BTreeMap::new()).unwrap();
    let destination = tempfile::tempdir().unwrap();
    unpack(&packed.bytes, destination.path(), &packed.sha256).unwrap();
    assert_eq!(
        fs::read_to_string(destination.path().join("alias")).unwrap(),
        "ok"
    );
    std::os::unix::fs::symlink("../outside", source.path().join("unsafe")).unwrap();
    assert!(pack(source.path(), &BTreeMap::new()).is_err());
}

#[test]
fn rejects_link_chain_escape_and_cycles() {
    let links = BTreeMap::from([(
        std::path::PathBuf::from("alias"),
        std::path::PathBuf::from("."),
    )]);
    assert!(
        validate_link_graph(Path::new("attack"), Path::new("alias/../outside"), &links).is_err()
    );
    let cycle = BTreeMap::from([
        (std::path::PathBuf::from("a"), std::path::PathBuf::from("b")),
        (std::path::PathBuf::from("b"), std::path::PathBuf::from("a")),
    ]);
    assert!(validate_link_graph(Path::new("a"), Path::new("b"), &cycle).is_err());
}
