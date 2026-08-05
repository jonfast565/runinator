//! parity between the routes the server actually serves and the routes it documents.
//!
//! the router registers 160-plus routes as typed axum handlers, while the openapi spec is built
//! from a separate hand-written `ENDPOINT_DOCS` table. nothing links them, so a route added to one
//! and not the other is invisible until a caller reads the spec and gets a 404 (or misses an
//! endpoint that exists).
//!
//! a `const ROUTES` table that both sides consume is not expressible here: each `.route()` call
//! carries a distinct handler type and its own `Extension` layer, so the registrations cannot be
//! reduced to data without boxing every handler and losing the typed extractors. instead this reads
//! both sources of truth as text and diffs them. it is a lint over the source, and it costs nothing
//! at runtime.

use std::collections::{BTreeMap, BTreeSet};

use super::openapi_document;

const ROUTER_SRC: &str = include_str!("router.rs");
const API_ROUTES_SRC: &str = include_str!("../../runinator-models/src/api_routes.rs");

/// http verbs axum route registrations use. an identifier only counts as a verb when it is called
/// as `verb(` at the head of the method router or chained as `.verb(`, so a handler named
/// `get_workflows` is never mistaken for a `get`.
const VERBS: &[&str] = &["get", "post", "put", "patch", "delete"];

/// routes that are intentionally undocumented, each with the reason.
///
/// these are infrastructure surfaces rather than api endpoints: they serve the docs themselves, or
/// speak a non-http protocol that openapi cannot describe.
/// note how short this is: the spec already documents the websocket upgrades and its own
/// `/openapi.json` and `/docs`, so an upgrade endpoint is not automatically excusable here.
const UNDOCUMENTED: &[(&str, &str)] = &[(
    "/ws/desktop-worker",
    "broker relay for the desktop agent, not a client-facing api",
)];

/// routes that *should* be documented but are not yet: recorded debt, not an exemption.
///
/// this list is a ratchet. `every_route_is_documented` fails for any route outside both lists, so
/// new drift is blocked the moment it appears; `pending_documentation_has_no_stale_entries` fails
/// once an entry gains documentation, so the list can only shrink. writing the `ENDPOINT_DOCS`
/// entries for these is content work, tracked separately — the guard exists so the number cannot
/// grow while that happens.
///
/// entire feature areas are missing here (pipelines, orgs, freeze windows, notification policies,
/// billing/quota), which is why the count is what it is.
const PENDING_DOCUMENTATION: &[(&str, &str)] = &[
    ("delete", "/artifacts/{id}"),
    ("delete", "/automation_events/{id}"),
    ("delete", "/freeze_windows/{id}"),
    ("delete", "/gates/{id}"),
    ("delete", "/notification_policies/{id}"),
    ("delete", "/notifications/{id}"),
    ("delete", "/orgs/{id}"),
    ("delete", "/orgs/{id}/members/{user_id}"),
    ("delete", "/pipeline_triggers/{id}"),
    ("delete", "/pipelines/{id}"),
    ("get", "/freeze_windows"),
    ("get", "/notification_policies"),
    ("get", "/notifications/{id}/deliveries"),
    ("get", "/orgs"),
    ("get", "/orgs/me"),
    ("get", "/orgs/{id}"),
    ("get", "/orgs/{id}/members"),
    ("get", "/orgs/{id}/nodes"),
    ("get", "/orgs/{id}/quota"),
    ("get", "/orgs/{id}/usage"),
    ("get", "/pipeline_runs"),
    ("get", "/pipeline_runs/{id}"),
    ("get", "/pipelines"),
    ("get", "/pipelines/{id}"),
    ("get", "/pipelines/{id}/triggers"),
    ("get", "/rate-card"),
    ("get", "/replicas/{replica_id}/samples"),
    ("get", "/workflow_runs/{id}/transitions"),
    ("get", "/workflows/{id}/nodes/{node_id}/transitions"),
    ("patch", "/freeze_windows/{id}"),
    ("patch", "/notification_policies/{id}"),
    ("patch", "/orgs/{id}"),
    ("patch", "/orgs/{id}/members/{user_id}"),
    ("patch", "/pipeline_triggers/{id}"),
    ("patch", "/pipelines/{id}"),
    ("patch", "/pipelines/{id}/owner"),
    ("patch", "/workflows/{id}/owner"),
    ("post", "/auth/switch-org"),
    ("post", "/freeze_windows"),
    ("post", "/idempotency_keys/claim"),
    ("post", "/idempotency_keys/complete"),
    ("post", "/idempotency_keys/release"),
    ("post", "/nodes/scale"),
    ("post", "/nodes/stop"),
    ("post", "/notification_policies"),
    ("post", "/orgs"),
    ("post", "/orgs/{id}/members"),
    ("post", "/orgs/{id}/nodes/scale"),
    ("post", "/pipeline_runs/{id}/cancel"),
    ("post", "/pipeline_triggers/{id}/runs"),
    ("post", "/pipelines"),
    ("post", "/pipelines/{id}/runs"),
    ("post", "/pipelines/{id}/triggers"),
    ("post", "/workflow_triggers/{id}/backfill"),
    ("post", "/workflows/simulate"),
    ("put", "/orgs/{id}/quota"),
];

/// `pub const NAME: &str = "value";` declarations from the shared api-route constants.
///
/// parsed rather than hand-mirrored so the map cannot itself drift.
fn route_constants() -> BTreeMap<String, String> {
    let mut constants = BTreeMap::new();
    let mut cursor = 0usize;

    // scanned over the whole text rather than line by line: rustfmt wraps a long declaration onto
    // the following line, and a line-based reader would silently skip exactly the longest paths.
    while let Some(offset) = API_ROUTES_SRC[cursor..].find("pub const ") {
        let start = cursor + offset + "pub const ".len();
        let Some(terminator) = API_ROUTES_SRC[start..].find(';') else {
            break;
        };
        let declaration = &API_ROUTES_SRC[start..start + terminator];
        cursor = start + terminator;

        let Some((name, value)) = declaration.split_once('=') else {
            continue;
        };
        let name = name
            .split(':')
            .next()
            .unwrap_or_default()
            .trim()
            .to_string();
        let value = value.trim();
        let Some(literal) = value.strip_prefix('"').and_then(|v| v.strip_suffix('"')) else {
            continue;
        };
        // only path constants participate; header names and other string constants do not start
        // with a slash and would never match a route.
        if literal.starts_with('/') {
            constants.insert(name, literal.to_string());
        }
    }
    constants
}

/// the byte range of the balanced parenthesis group starting at `open`, which must index a `(`.
fn balanced(src: &str, open: usize) -> Option<(usize, usize)> {
    let bytes = src.as_bytes();
    let mut depth = 0usize;
    let mut in_string = false;
    let mut index = open;
    while index < bytes.len() {
        let byte = bytes[index];
        if in_string {
            match byte {
                b'\\' => index += 1,
                b'"' => in_string = false,
                _ => {}
            }
        } else {
            match byte {
                b'"' => in_string = true,
                b'(' => depth += 1,
                b')' => {
                    depth -= 1;
                    if depth == 0 {
                        return Some((open + 1, index));
                    }
                }
                _ => {}
            }
        }
        index += 1;
    }
    None
}

/// split an argument list at top-level commas.
fn split_args(args: &str) -> Vec<&str> {
    let bytes = args.as_bytes();
    let mut parts = Vec::new();
    let (mut depth, mut in_string, mut start) = (0i32, false, 0usize);
    for (index, byte) in bytes.iter().enumerate() {
        if in_string {
            if *byte == b'"' {
                in_string = false;
            }
            continue;
        }
        match byte {
            b'"' => in_string = true,
            b'(' | b'[' | b'<' => depth += 1,
            b')' | b']' | b'>' => depth -= 1,
            b',' if depth == 0 => {
                parts.push(args[start..index].trim());
                start = index + 1;
            }
            _ => {}
        }
    }
    parts.push(args[start..].trim());
    parts
}

/// verbs invoked in a method-router expression: the leading `verb(...)` plus any chained `.verb(`.
fn verbs_in(expr: &str) -> BTreeSet<String> {
    let mut found = BTreeSet::new();
    for verb in VERBS {
        let head = format!("{verb}(");
        // the leading call, either bare (`get(handler)`) or fully qualified
        // (`axum::routing::patch(handler)`).
        if expr.starts_with(&head) || expr.contains(&format!("routing::{head}")) {
            found.insert((*verb).to_string());
        }
        if expr.contains(&format!(".{head}")) {
            found.insert((*verb).to_string());
        }
    }
    found
}

/// every `(method, path)` the router registers.
fn registered_routes() -> BTreeSet<(String, String)> {
    let constants = route_constants();
    let mut routes = BTreeSet::new();
    let mut cursor = 0usize;

    while let Some(offset) = ROUTER_SRC[cursor..].find(".route(") {
        let open = cursor + offset + ".route".len();
        let Some((start, end)) = balanced(ROUTER_SRC, open) else {
            break;
        };
        let args = split_args(&ROUTER_SRC[start..end]);
        cursor = end;

        let (Some(path_arg), Some(method_arg)) = (args.first(), args.get(1)) else {
            continue;
        };
        // a path is either a literal or one of the shared `API_*` constants.
        let path = match path_arg.strip_prefix('"').and_then(|p| p.strip_suffix('"')) {
            Some(literal) => literal.to_string(),
            None => match constants.get(*path_arg) {
                Some(resolved) => resolved.clone(),
                // an unresolvable path expression would silently drop a route from this guard.
                None => panic!(
                    "route path `{path_arg}` is neither a string literal nor a known constant in \
                     runinator-models/src/api_routes.rs; this test cannot see the route"
                ),
            },
        };
        for verb in verbs_in(method_arg) {
            routes.insert((verb, path.clone()));
        }
    }
    routes
}

/// every `(method, path)` the generated openapi document describes.
fn documented_routes() -> BTreeSet<(String, String)> {
    let document = openapi_document();
    let mut documented = BTreeSet::new();
    let Some(paths) = document["paths"].as_object() else {
        return documented;
    };
    for (path, item) in paths {
        let Some(operations) = item.as_object() else {
            continue;
        };
        for method in operations.keys() {
            if VERBS.contains(&method.as_str()) {
                documented.insert((method.clone(), path.clone()));
            }
        }
    }
    documented
}

/// the parser sees the routes that are really there.
///
/// guards the guard: a silently-failing extractor would make the parity test below vacuous.
#[test]
fn router_source_parses_into_routes() {
    let routes = registered_routes();
    assert!(
        routes.len() > 150,
        "expected the router to register 150+ routes, found {}; the source parser has probably \
         stopped matching",
        routes.len()
    );
    for probe in [
        ("get", "/health"),
        ("post", "/workflows/validate"),
        ("patch", "/workflows/{id}"),
        ("delete", "/workflows/{id}"),
    ] {
        let probe = (probe.0.to_string(), probe.1.to_string());
        assert!(routes.contains(&probe), "router parser missed {probe:?}");
    }
}

/// every route the server serves is either documented or explicitly excused.
#[test]
fn every_route_is_documented() {
    let documented = documented_routes();
    let mut missing: Vec<String> = registered_routes()
        .into_iter()
        .filter(|route| !documented.contains(route))
        .filter(|(_, path)| !UNDOCUMENTED.iter().any(|(excused, _)| excused == path))
        .filter(|(method, path)| {
            !PENDING_DOCUMENTATION
                .iter()
                .any(|(pending_method, pending_path)| {
                    pending_method == method && pending_path == path
                })
        })
        .map(|(method, path)| format!("{} {path}", method.to_uppercase()))
        .collect();
    missing.sort();

    assert!(
        missing.is_empty(),
        "these routes are served but absent from the openapi spec; add an ENDPOINT_DOCS entry in \
         openapi.rs, or list the path in UNDOCUMENTED with a reason:\n  {}",
        missing.join("\n  ")
    );
}

/// the documentation-debt list only shrinks.
///
/// an entry that has since been documented must be removed, so the list stays an accurate count of
/// what is still missing rather than decaying into a permanent exemption.
#[test]
fn pending_documentation_has_no_stale_entries() {
    let documented = documented_routes();
    let registered = registered_routes();
    let mut stale = Vec::new();

    for (method, path) in PENDING_DOCUMENTATION {
        let route = ((*method).to_string(), (*path).to_string());
        if documented.contains(&route) {
            stale.push(format!(
                "{} {path} is documented now; remove it from PENDING_DOCUMENTATION",
                method.to_uppercase()
            ));
        }
        if !registered.contains(&route) {
            stale.push(format!(
                "{} {path} is no longer served; remove it from PENDING_DOCUMENTATION",
                method.to_uppercase()
            ));
        }
    }

    assert!(stale.is_empty(), "{}", stale.join("\n  "));
}

/// every documented route is actually served.
///
/// the more dangerous direction: a caller who trusts the spec gets a 404.
#[test]
fn every_documented_route_exists() {
    let registered = registered_routes();
    let mut phantom: Vec<String> = documented_routes()
        .into_iter()
        .filter(|route| !registered.contains(route))
        .map(|(method, path)| format!("{} {path}", method.to_uppercase()))
        .collect();
    phantom.sort();

    assert!(
        phantom.is_empty(),
        "these routes are documented in openapi.rs but not registered in router.rs, so callers \
         following the spec get a 404:\n  {}",
        phantom.join("\n  ")
    );
}

/// nothing lingers in `UNDOCUMENTED` after it stops being a route or gains documentation.
#[test]
fn undocumented_allowlist_has_no_stale_entries() {
    let registered: BTreeSet<String> = registered_routes()
        .into_iter()
        .map(|(_, path)| path)
        .collect();
    let documented: BTreeSet<String> = documented_routes()
        .into_iter()
        .map(|(_, path)| path)
        .collect();

    for (path, reason) in UNDOCUMENTED {
        assert!(
            registered.contains(*path),
            "UNDOCUMENTED lists {path} ({reason}) but the router no longer serves it"
        );
        assert!(
            !documented.contains(*path),
            "UNDOCUMENTED lists {path} ({reason}) but it is documented now; drop the entry"
        );
    }
}
