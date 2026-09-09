//! Portable path and link confinement.
use super::*;

#[test]
fn rejects_parent_traversal_absolute_paths_and_escaping_links() {
    for path in ["", "../outside", "/outside", "a/../outside"] {
        assert!(validate_path(Path::new(path)).is_err());
    }
    assert!(validate_link(Path::new("a/link"), Path::new("../../outside")).is_err());
    assert!(validate_link(Path::new("a/link"), Path::new("../inside")).is_ok());
}

#[test]
fn rejects_link_chain_escape_and_cycles() {
    let links = BTreeMap::from([
        (Path::new("a/link").into(), Path::new("../b").into()),
        (Path::new("b").into(), Path::new("../outside").into()),
    ]);
    assert!(validate_link_graph(Path::new("a/link"), Path::new("../b"), &links).is_err());
    let links = BTreeMap::from([
        (Path::new("a").into(), Path::new("b").into()),
        (Path::new("b").into(), Path::new("a").into()),
    ]);
    assert!(validate_link_graph(Path::new("a"), Path::new("b"), &links).is_err());
}
