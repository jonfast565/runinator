//! covers the package identity key. the lifecycle itself lives in `dialect_parity`, so that every
//! engine runs it; this covers only the pure function that decides when two packages are the same.

use super::*;

#[test]
fn distinguishes_packages_across_orgs_and_namespaces() {
    let org = Uuid::from_u128(1);
    let other = Uuid::from_u128(2);

    assert_ne!(
        identity_key(Some(org), None, "tools"),
        identity_key(Some(other), None, "tools"),
        "two orgs may each own a package called `tools`"
    );
    assert_ne!(
        identity_key(None, None, "tools"),
        identity_key(Some(org), None, "tools"),
        "a platform-global package is not an org's package"
    );
    assert_ne!(
        identity_key(None, Some("media"), "tools"),
        identity_key(None, None, "tools"),
        "a namespaced package is not the unqualified one"
    );
}

#[test]
fn a_dotted_namespace_cannot_collide_with_a_dotted_name() {
    // the separator is what makes this safe: joined with `.`, `media` + `image.tools` and
    // `media.image` + `tools` would render the same key and silently become one package.
    assert_ne!(
        identity_key(None, Some("media"), "image.tools"),
        identity_key(None, Some("media.image"), "tools")
    );
}

#[test]
fn is_stable_for_the_same_identity() {
    let org = Uuid::from_u128(7);
    assert_eq!(
        identity_key(Some(org), Some("media"), "tools"),
        identity_key(Some(org), Some("media"), "tools")
    );
    // an absent namespace and an empty one name the same package, since neither qualifies the name.
    assert_eq!(
        identity_key(None, None, "tools"),
        identity_key(None, Some(""), "tools")
    );
}
