//! covers the resource uri shapes and the templates advertised for them. reading one needs a web
//! service, so what is asserted here is the addressing.

use super::*;

fn uris(resources: &[Value]) -> Vec<String> {
    resources
        .iter()
        .filter_map(|resource| resource.get("uri").and_then(Value::as_str))
        .map(str::to_string)
        .collect()
}

#[test]
fn every_addressable_shape_is_advertised() {
    let uris = uris(&templates());
    assert!(uris.contains(&"runinator://workflows".to_string()));
    assert!(uris.contains(&"runinator://workflows/{id}".to_string()));
    assert!(uris.contains(&"runinator://runs/{id}".to_string()));
    assert!(uris.contains(&"runinator://runs/{id}/artifacts".to_string()));
    assert!(uris.contains(&"runinator://effects/{id}/output".to_string()));
}

#[test]
fn every_template_names_itself_and_its_type() {
    for template in templates() {
        assert!(
            template
                .get("name")
                .and_then(Value::as_str)
                .is_some_and(|name| !name.is_empty()),
            "{template:?} has no name"
        );
        assert_eq!(
            template.get("mimeType").and_then(Value::as_str),
            Some("application/json")
        );
    }
}

#[test]
fn a_uuid_is_read_out_of_the_uri_it_sits_in() {
    let id = "8f14e45f-ceea-467a-9a2c-8d1e4d1c9b21";
    assert_eq!(
        uuid_after(&format!("{RUN_PREFIX}{id}"), RUN_PREFIX, ""),
        Some(id.parse().unwrap())
    );
    assert_eq!(
        uuid_after(
            &format!("{EFFECT_PREFIX}{id}/output"),
            EFFECT_PREFIX,
            "/output"
        ),
        Some(id.parse().unwrap())
    );
}

// `runs/{id}` and `runs/{id}/artifacts` are different resources, and the suffix is what tells them
// apart — a prefix match alone would read the artifacts uri as a malformed run id.
#[test]
fn a_suffixed_uri_does_not_match_the_bare_shape() {
    let id = "8f14e45f-ceea-467a-9a2c-8d1e4d1c9b21";
    assert_eq!(
        uuid_after(&format!("{RUN_PREFIX}{id}/artifacts"), RUN_PREFIX, ""),
        None
    );
    assert!(
        uuid_after(
            &format!("{RUN_PREFIX}{id}/artifacts"),
            RUN_PREFIX,
            "/artifacts"
        )
        .is_some()
    );
}

#[test]
fn a_uri_that_is_not_this_shape_reads_as_nothing() {
    assert_eq!(
        uuid_after("runinator://runs/not-a-uuid", RUN_PREFIX, ""),
        None
    );
    assert_eq!(
        uuid_after("https://example.com/runs/x", RUN_PREFIX, ""),
        None
    );
    assert_eq!(uuid_after(RUN_PREFIX, RUN_PREFIX, ""), None);
}
