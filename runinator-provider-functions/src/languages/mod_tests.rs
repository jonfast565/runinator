//! covers runtime-string resolution, since a manifest's `runtime` is the one thing here that is
//! author-supplied and has to survive versions this build has never seen.

use super::*;

#[test]
fn resolves_a_family_and_carries_its_version_through() {
    let (adapter, version) = adapter_for("python3.13").unwrap();
    assert_eq!(adapter.family(), "python");
    assert_eq!(version, "3.13");
    assert_eq!(adapter.default_image(&version), "python:3.13-slim");

    let (adapter, version) = adapter_for("node22").unwrap();
    assert_eq!(adapter.family(), "node");
    assert_eq!(adapter.default_image(&version), "node:22-slim");
}

#[test]
fn a_bare_family_falls_back_to_a_default_version() {
    let (adapter, version) = adapter_for("python").unwrap();
    assert!(version.is_empty());
    assert_eq!(adapter.default_image(&version), "python:3.13-slim");
}

#[test]
fn an_unknown_version_still_resolves_to_an_image() {
    // the version is carried through rather than matched against a list, so a package naming a
    // version newer than this build still runs instead of failing on our ignorance of it.
    let (adapter, version) = adapter_for("python3.99").unwrap();
    assert_eq!(adapter.default_image(&version), "python:3.99-slim");
}

#[test]
fn an_unsupported_runtime_is_refused_by_name() {
    let error = match adapter_for("cobol85") {
        Ok(_) => panic!("cobol should not resolve"),
        Err(error) => error.to_string(),
    };
    assert!(error.contains("FUNC002"));
    assert!(error.contains("cobol"));
}

#[test]
fn aliases_resolve_to_the_same_adapter() {
    for alias in ["py3.13", "python3.13", "PYTHON3.13"] {
        assert_eq!(adapter_for(alias).unwrap().0.family(), "python");
    }
    for alias in ["js", "javascript", "nodejs20"] {
        assert_eq!(adapter_for(alias).unwrap().0.family(), "node");
    }
}
