//! which handlers are allowed to touch the store directly.
//!
//! `AGENTS.md` says persistence orchestration lives in `runinator-engine/src/repository/`, with a
//! closed exemption for thin CRUD over rows the runtime never orchestrates (auth, orgs, billing)
//! plus the readiness probe. Nothing in the type system enforces that: every handler already holds
//! an `Arc<T: DatabaseImpl>` so it can pass `db.as_ref()` to a repository function, and calling a
//! store method on it instead compiles just as well.
//!
//! this is a lint over the handler sources. it is a ratchet in both directions — a new direct call
//! in an unlisted handler fails, and so does an allowlist entry whose file stopped making one, so
//! the list cannot rot into a blanket exemption.

use std::collections::BTreeSet;
use std::fs;
use std::path::Path;

/// handlers that may call `db.*` themselves, each with the reason it is not orchestration.
const DIRECT_STORE_ACCESS: &[(&str, &str)] = &[
    (
        "auth.rs",
        "users, api keys, sessions: crud the runtime never drives",
    ),
    (
        "orgs.rs",
        "orgs, memberships, quotas: crud the runtime never drives",
    ),
    (
        "billing.rs",
        "plans and invoices: crud the runtime never drives",
    ),
    (
        "credentials.rs",
        "credential rows: crud the runtime never drives",
    ),
    (
        "catalog.rs",
        "catalog item rows: crud the runtime never drives",
    ),
    (
        "health.rs",
        "readiness probe: reads one row to test connectivity, ignores its content",
    ),
];

/// `db.foo(` and `db.as_ref().foo(` are store calls; `db.as_ref()` and `db.clone()` on their own are
/// how a handler hands the store to a repository function, which is the sanctioned path.
fn calls_store_directly(source: &str) -> bool {
    source.lines().any(|line| {
        let mut rest = line;
        while let Some(at) = rest.find("db.") {
            // require a word boundary so `self.db.` or `worker_db.` are not read as the extension.
            let preceded_by_word = rest[..at]
                .chars()
                .next_back()
                .is_some_and(|ch| ch.is_alphanumeric() || ch == '_' || ch == '.');
            let tail = &rest[at + 3..];
            rest = tail;
            if preceded_by_word {
                continue;
            }
            let tail = tail.strip_prefix("as_ref().").unwrap_or(tail);
            let method: String = tail
                .chars()
                .take_while(|ch| ch.is_alphanumeric() || *ch == '_')
                .collect();
            if method.is_empty() || !tail[method.len()..].starts_with('(') {
                continue;
            }
            if method == "as_ref" || method == "clone" {
                continue;
            }
            return true;
        }
        false
    })
}

fn offenders() -> BTreeSet<String> {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("src/handlers");
    let entries = fs::read_dir(&dir).expect("handlers directory readable");
    let mut found = BTreeSet::new();
    for entry in entries {
        let path = entry.expect("readable dir entry").path();
        if path.extension().and_then(|ext| ext.to_str()) != Some("rs") {
            continue;
        }
        let name = path
            .file_name()
            .and_then(|name| name.to_str())
            .expect("utf-8 file name")
            .to_string();
        if name == "mod.rs" || name.ends_with("_tests.rs") {
            continue;
        }
        let source = fs::read_to_string(&path).expect("handler source readable");
        if calls_store_directly(&source) {
            found.insert(name);
        }
    }
    found
}

#[test]
fn only_allowlisted_handlers_call_the_store_directly() {
    let allowed: BTreeSet<String> = DIRECT_STORE_ACCESS
        .iter()
        .map(|(name, _)| (*name).to_string())
        .collect();
    let found = offenders();
    let unexpected: Vec<&String> = found.difference(&allowed).collect();
    assert!(
        unexpected.is_empty(),
        "these handlers call the store directly but are not exempt: {unexpected:?}. \
         persistence orchestration belongs in runinator-engine/src/repository/ — see the \
         'When a ws handler may call the store directly' section of AGENTS.md."
    );
}

#[test]
fn store_access_allowlist_has_no_stale_entries() {
    let found = offenders();
    let stale: Vec<&str> = DIRECT_STORE_ACCESS
        .iter()
        .map(|(name, _)| *name)
        .filter(|name| !found.contains(*name))
        .collect();
    assert!(
        stale.is_empty(),
        "these handlers no longer call the store directly; drop them from DIRECT_STORE_ACCESS so \
         the exemption stays a closed list: {stale:?}"
    );
}

/// the lint is only worth its allowlist if it actually fires, so pin the detector against the two
/// shapes it must tell apart: handing the store to a repository function versus calling it.
#[test]
fn detector_separates_delegation_from_direct_calls() {
    assert!(!calls_store_directly(
        "    match repository::fetch_run_artifacts(db.as_ref(), run_id).await {"
    ));
    assert!(!calls_store_directly(
        "    repository::claim_pending_action_dispatches(\n        db.as_ref(),"
    ));
    assert!(calls_store_directly(
        "    db.fetch_notifications(true, 10).await"
    ));
    assert!(calls_store_directly(
        "    db.as_ref().mark_notification_read(id).await"
    ));
    // a differently-named store handle is not the `db` extension this rule is about.
    assert!(!calls_store_directly(
        "    worker_db.fetch_notifications(true, 10).await"
    ));
}

#[test]
fn every_exemption_states_a_reason() {
    for (name, reason) in DIRECT_STORE_ACCESS {
        assert!(
            reason.len() > 20,
            "{name} needs a real reason for its exemption, not {reason:?}"
        );
    }
}
