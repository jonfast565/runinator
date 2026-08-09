use super::{allow_interactive, working_dir};
use std::path::PathBuf;

#[test]
fn interactive_gate_reads_env_flag() {
    // permitted only for a non-empty, non-"0" flag; unset/empty/"0" reject (cloud-worker default).
    assert!(allow_interactive(Some("1")));
    assert!(allow_interactive(Some("true")));
    assert!(!allow_interactive(Some("0")));
    assert!(!allow_interactive(Some("")));
    assert!(!allow_interactive(None));
}

#[test]
fn working_dir_reads_env_path() {
    // a non-empty, trimmed path is used; unset/empty/blank inherit the process cwd (None).
    assert_eq!(
        working_dir(Some("/tmp/work")),
        Some(PathBuf::from("/tmp/work"))
    );
    assert_eq!(
        working_dir(Some("  /tmp/work  ")),
        Some(PathBuf::from("/tmp/work"))
    );
    assert_eq!(working_dir(Some("")), None);
    assert_eq!(working_dir(Some("   ")), None);
    assert_eq!(working_dir(None), None);
}
