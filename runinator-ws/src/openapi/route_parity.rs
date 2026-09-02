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
//!
//! registration is spread across one `routes()` fn per handler module — and those modules now live
//! in three sibling crates — so the "source" side is [`ROUTER_SOURCES`] rather than a single file.
//! that list is itself a thing that can rot — a module missing from it would drop its routes out of
//! this guard entirely — so [`route_sources_cover_every_module`] reads every handler directory in
//! [`HANDLER_CRATES`] and fails if one is unlisted.
//!
//! this is the one place that still sees the whole surface at once: the split put each domain's
//! routes and docs in its own crate, and nothing there can tell whether the *merged* router matches
//! the *merged* spec. that is why the guard stays here, reaching across crate boundaries by path,
//! rather than being cut into three per-crate lints that each check only their own slice.

use std::collections::{BTreeMap, BTreeSet};

use super::{MINIMAL_ENDPOINTS, openapi_document};

use crate::{HANDLER_CRATES, workspace_root};

/// every module that registers routes, as `(module path, source)`.
///
/// `include_str!` cannot be globbed and cannot reach a crate by name, so the handler entries are
/// written out as relative paths into the sibling crates; `route_sources_cover_every_module` checks
/// the list against those directories rather than trusting it.
const ROUTER_SOURCES: &[(&str, &str)] = &[
    ("websocket", include_str!("../websocket.rs")),
    ("openapi", include_str!("mod.rs")),
    (
        "runinator-ws-identity/handlers/auth",
        include_str!("../../../runinator-ws-identity/src/handlers/auth.rs"),
    ),
    (
        "runinator-ws-identity/handlers/authz",
        include_str!("../../../runinator-ws-identity/src/handlers/authz.rs"),
    ),
    (
        "runinator-ws-identity/handlers/billing",
        include_str!("../../../runinator-ws-identity/src/handlers/billing.rs"),
    ),
    (
        "runinator-ws-identity/handlers/orgs",
        include_str!("../../../runinator-ws-identity/src/handlers/orgs.rs"),
    ),
    (
        "runinator-ws-authoring/handlers/catalog",
        include_str!("../../../runinator-ws-authoring/src/handlers/catalog.rs"),
    ),
    (
        "runinator-ws-authoring/handlers/adapters",
        include_str!("../../../runinator-ws-authoring/src/handlers/adapters.rs"),
    ),
    (
        "runinator-ws-authoring/handlers/console",
        include_str!("../../../runinator-ws-authoring/src/handlers/console.rs"),
    ),
    (
        "runinator-ws-authoring/handlers/credentials",
        include_str!("../../../runinator-ws-authoring/src/handlers/credentials.rs"),
    ),
    (
        "runinator-ws-authoring/handlers/functions",
        include_str!("../../../runinator-ws-authoring/src/handlers/functions.rs"),
    ),
    (
        "runinator-ws-authoring/handlers/packs",
        include_str!("../../../runinator-ws-authoring/src/handlers/packs.rs"),
    ),
    (
        "runinator-ws-authoring/handlers/pipelines",
        include_str!("../../../runinator-ws-authoring/src/handlers/pipelines.rs"),
    ),
    (
        "runinator-ws-authoring/handlers/orchestrations",
        include_str!("../../../runinator-ws-authoring/src/handlers/orchestrations.rs"),
    ),
    (
        "runinator-ws-authoring/handlers/providers",
        include_str!("../../../runinator-ws-authoring/src/handlers/providers.rs"),
    ),
    (
        "runinator-ws-authoring/handlers/rexrap",
        include_str!("../../../runinator-ws-authoring/src/handlers/rexrap.rs"),
    ),
    (
        "runinator-ws-authoring/handlers/workflows",
        include_str!("../../../runinator-ws-authoring/src/handlers/workflows.rs"),
    ),
    (
        "runinator-ws-runtime/handlers/agents",
        include_str!("../../../runinator-ws-runtime/src/handlers/agents.rs"),
    ),
    (
        "runinator-ws-runtime/handlers/artifacts",
        include_str!("../../../runinator-ws-runtime/src/handlers/artifacts.rs"),
    ),
    (
        "runinator-ws-runtime/handlers/automation",
        include_str!("../../../runinator-ws-runtime/src/handlers/automation.rs"),
    ),
    (
        "runinator-ws-runtime/handlers/catalog_metadata",
        include_str!("../../../runinator-ws-runtime/src/handlers/catalog_metadata.rs"),
    ),
    (
        "runinator-ws-runtime/handlers/debug",
        include_str!("../../../runinator-ws-runtime/src/handlers/debug.rs"),
    ),
    (
        "runinator-ws-runtime/handlers/files",
        include_str!("../../../runinator-ws-runtime/src/handlers/files.rs"),
    ),
    (
        "runinator-ws-runtime/handlers/function_invocations",
        include_str!("../../../runinator-ws-runtime/src/handlers/function_invocations.rs"),
    ),
    (
        "runinator-ws-runtime/handlers/health",
        include_str!("../../../runinator-ws-runtime/src/handlers/health.rs"),
    ),
    (
        "runinator-ws-runtime/handlers/ingress_control",
        include_str!("../../../runinator-ws-runtime/src/handlers/ingress_control.rs"),
    ),
    (
        "runinator-ws-runtime/handlers/notifications",
        include_str!("../../../runinator-ws-runtime/src/handlers/notifications.rs"),
    ),
    (
        "runinator-ws-runtime/handlers/observability",
        include_str!("../../../runinator-ws-runtime/src/handlers/observability.rs"),
    ),
    (
        "runinator-ws-runtime/handlers/provisioning",
        include_str!("../../../runinator-ws-runtime/src/handlers/provisioning.rs"),
    ),
    (
        "runinator-ws-runtime/handlers/replicas",
        include_str!("../../../runinator-ws-runtime/src/handlers/replicas.rs"),
    ),
    (
        "runinator-ws-runtime/handlers/runs",
        include_str!("../../../runinator-ws-runtime/src/handlers/runs.rs"),
    ),
    (
        "runinator-ws-runtime/handlers/schedules",
        include_str!("../../../runinator-ws-runtime/src/handlers/schedules.rs"),
    ),
    (
        "runinator-ws-runtime/handlers/supervisor",
        include_str!("../../../runinator-ws-runtime/src/handlers/supervisor.rs"),
    ),
    (
        "runinator-ws-runtime/handlers/triggers",
        include_str!("../../../runinator-ws-runtime/src/handlers/triggers.rs"),
    ),
    (
        "runinator-ws-runtime/handlers/workflow_vm",
        include_str!("../../../runinator-ws-runtime/src/handlers/workflow_vm.rs"),
    ),
];

const API_ROUTES_SRC: &str = include_str!("../../../runinator-models/src/api_routes.rs");

/// the `API_*` consts are referenced by full path from the handler modules, so the bare name this
/// resolves against has to be recovered before looking it up.
const API_ROUTES_PREFIX: &str = "runinator_models::api_routes::";

/// http verbs axum route registrations use. an identifier only counts as a verb when it is called
/// as `verb(` at the head of the method router or chained as `.verb(`, so a handler named
/// `get_workflows` is never mistaken for a `get`.
const VERBS: &[&str] = &["get", "post", "put", "patch", "delete"];

/// routes that are intentionally undocumented, each with the reason.
///
/// these are infrastructure surfaces rather than API endpoints: they serve the docs themselves, or
/// speak a non-http protocol that openapi cannot describe.
/// note how short this is: the spec already documents the WS upgrades and its own
/// `/openapi.json` and `/docs`, so an upgrade endpoint is not automatically excusable here.
const UNDOCUMENTED: &[(&str, &str)] = &[];

/// `pub const NAME: &str = "value";` declarations from the shared API-route constants.
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

/// every `(method, path)` the router registers, across all of [`ROUTER_SOURCES`].
fn registered_routes() -> BTreeSet<(String, String)> {
    let constants = route_constants();
    let mut routes = BTreeSet::new();

    for (module, src) in ROUTER_SOURCES {
        let mut cursor = 0usize;
        while let Some(offset) = src[cursor..].find(".route(") {
            let open = cursor + offset + ".route".len();
            let Some((start, end)) = balanced(src, open) else {
                break;
            };
            let args = split_args(&src[start..end]);
            cursor = end;

            let (Some(path_arg), Some(method_arg)) = (args.first(), args.get(1)) else {
                continue;
            };
            // a path is either a literal or one of the shared `API_*` constants.
            let path = match path_arg.strip_prefix('"').and_then(|p| p.strip_suffix('"')) {
                Some(literal) => literal.to_string(),
                None => {
                    let name = path_arg.strip_prefix(API_ROUTES_PREFIX).unwrap_or(path_arg);
                    match constants.get(name) {
                        Some(resolved) => resolved.clone(),
                        // an unresolvable path expression would silently drop a route from this guard.
                        None => panic!(
                            "route path `{path_arg}` in {module} is neither a string literal nor a \
                             known constant in runinator-models/src/api_routes.rs; this test cannot \
                             see the route"
                        ),
                    }
                }
            };
            for verb in verbs_in(method_arg) {
                routes.insert((verb, path.clone()));
            }
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
        .map(|(method, path)| format!("{} {path}", method.to_uppercase()))
        .collect();
    missing.sort();

    assert!(
        missing.is_empty(),
        "these routes are served but absent from the openapi spec; add an ENDPOINT_DOCS entry in \
         `DOCS` slice, or list the path in UNDOCUMENTED with a reason:\n  {}",
        missing.join("\n  ")
    );
}

/// Minimal policy declarations are real OpenAPI operations and cannot outlive their routes.
#[test]
fn minimal_policy_declarations_have_no_stale_entries() {
    let documented = documented_routes();
    let registered = registered_routes();
    let mut stale = Vec::new();

    for (method, path, _) in MINIMAL_ENDPOINTS {
        let route = ((*method).to_string(), (*path).to_string());
        if !documented.contains(&route) {
            stale.push(format!(
                "{} {path} has no OpenAPI operation",
                method.to_uppercase()
            ));
        }
        if !registered.contains(&route) {
            stale.push(format!(
                "{} {path} is no longer served; remove its minimal policy declaration",
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
        "these routes are documented but no module registers them, so callers \
         following the spec get a 404:\n  {}",
        phantom.join("\n  ")
    );
}

/// `ROUTER_SOURCES` names every module that registers routes.
///
/// registration is per-module now, so this list is the seam the whole guard hangs from: a handler
/// module that grows a `routes()` fn without being added here would have all of its routes drop out
/// of `registered_routes`, and `every_route_is_documented` would pass while documenting nothing.
/// `include_str!` takes a literal, so the list cannot be globbed — it is checked instead.
#[test]
fn route_sources_cover_every_module() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    let workspace = workspace_root();
    let listed: BTreeSet<&str> = ROUTER_SOURCES.iter().map(|(module, _)| *module).collect();

    let mut registrars = BTreeSet::new();
    for crate_name in HANDLER_CRATES {
        let dir = workspace.join(crate_name).join("src").join("handlers");
        for entry in std::fs::read_dir(&dir)
            .unwrap_or_else(|err| panic!("{crate_name} handlers dir is readable: {err}"))
        {
            let path = entry.expect("handler dir entry").path();
            if path.extension().is_none_or(|ext| ext != "rs") {
                continue;
            }
            let stem = path.file_stem().unwrap_or_default().to_string_lossy();
            let source = std::fs::read_to_string(&path).expect("handler source is readable");
            if source.contains("fn routes") {
                registrars.insert(format!("{crate_name}/handlers/{stem}"));
            }
        }
    }
    // the two registrars outside `handlers/`.
    for (module, file) in [("websocket", "websocket.rs"), ("openapi", "openapi/mod.rs")] {
        let source = std::fs::read_to_string(root.join(file)).expect("source is readable");
        assert!(
            source.contains("fn routes"),
            "{module} no longer registers routes; drop it from ROUTER_SOURCES"
        );
        registrars.insert(module.to_string());
    }

    let unlisted: Vec<&String> = registrars
        .iter()
        .filter(|module| !listed.contains(module.as_str()))
        .collect();
    assert!(
        unlisted.is_empty(),
        "these modules register routes but are missing from ROUTER_SOURCES, so their routes are \
         invisible to this guard; add an include_str! entry for each:\n  {unlisted:?}"
    );

    let stale: Vec<&str> = listed
        .iter()
        .filter(|module| !registrars.contains(**module))
        .copied()
        .collect();
    assert!(
        stale.is_empty(),
        "ROUTER_SOURCES lists modules that no longer register routes: {stale:?}"
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
