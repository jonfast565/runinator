//! atomic secret-file persistence.

use super::*;

#[test]
fn replaces_a_secret_without_leaving_a_temp_file() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("agent.key");
    write_secret_file_atomic(&path, b"first").unwrap();
    write_secret_file_atomic(&path, b"second").unwrap();
    assert_eq!(std::fs::read(&path).unwrap(), b"second");
    assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 1);
}
