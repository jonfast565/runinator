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
//!
//! the handlers live in three sibling crates now, so entries are keyed by `<crate>/<file>` and the
//! scan walks each crate in [`crate::HANDLER_CRATES`]. the qualified key is not decoration: it keeps
//! two crates from ever sharing an exemption through a coincidentally identical filename.

use std::collections::BTreeSet;
use std::fs;

use crate::{HANDLER_CRATES, workspace_root};

/// handlers that may call `db.*` themselves, each with the reason it is not orchestration.
const DIRECT_STORE_ACCESS: &[(&str, &str)] = &[
    (
        "runinator-ws-identity/auth.rs",
        "users, api keys, sessions: crud the runtime never drives",
    ),
    (
        "runinator-ws-identity/authz.rs",
        "role assignments, service accounts, ownership and grants are identity-domain CRUD",
    ),
    (
        "runinator-ws-identity/orgs.rs",
        "orgs, memberships, quotas: crud the runtime never drives",
    ),
    (
        "runinator-ws-identity/billing.rs",
        "plans and invoices: crud the runtime never drives",
    ),
    (
        "runinator-ws-authoring/credentials.rs",
        "credential rows: crud the runtime never drives",
    ),
    (
        "runinator-ws-authoring/catalog.rs",
        "startup-only built-in catalog seeding writes static metadata before HTTP serves requests",
    ),
    (
        "runinator-ws-runtime/health.rs",
        "readiness probe: reads one row to test connectivity, ignores its content",
    ),
    (
        "runinator-ws-runtime/workflow_vm.rs",
        "operator VM reads and effect settlement use durable VM rows, not legacy orchestration records",
    ),
];

/// Handler modules that completed the service migration. They may authorize a request against the
/// store, but must not reach back into the engine repository facade; that would put persistence,
/// event publication, and transport side effects back into HTTP glue.
const SERVICE_BACKED_HANDLERS: &[(&str, &str)] = &[
    ("runinator-ws-authoring/adapters.rs", "AdapterOperations"),
    (
        "runinator-ws-authoring/orchestrations.rs",
        "OrchestrationOperations",
    ),
    ("runinator-ws-authoring/functions.rs", "FunctionPackages"),
    ("runinator-ws-authoring/catalog.rs", "CatalogOperations"),
    ("runinator-ws-authoring/console.rs", "ConsoleOperations"),
    ("runinator-ws-authoring/pipelines.rs", "PipelineOperations"),
    ("runinator-ws-authoring/packs.rs", "PackOperations"),
    ("runinator-ws-authoring/workflows.rs", "WorkflowAuthoring"),
    ("runinator-ws-authoring/providers.rs", "CatalogOperations"),
    ("runinator-ws-authoring/rexrap.rs", "WorkflowAuthoring"),
    ("runinator-ws-runtime/automation.rs", "AutomationOperations"),
    ("runinator-ws-runtime/debug.rs", "DebugOperations"),
    (
        "runinator-ws-runtime/function_invocations.rs",
        "FunctionInvocations",
    ),
    ("runinator-ws-runtime/artifacts.rs", "WorkflowFiles"),
    (
        "runinator-ws-runtime/notifications.rs",
        "NotificationOperations",
    ),
    ("runinator-ws-runtime/replicas.rs", "ReplicaRegistry"),
    ("runinator-ws-runtime/runs.rs", "RunOperations"),
    ("runinator-ws-runtime/schedules.rs", "SchedulingOperations"),
    ("runinator-ws-runtime/triggers.rs", "SchedulingOperations"),
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
    let workspace = workspace_root();
    let mut found = BTreeSet::new();
    for crate_name in HANDLER_CRATES {
        let dir = workspace.join(crate_name).join("src").join("handlers");
        let entries = fs::read_dir(&dir)
            .unwrap_or_else(|err| panic!("{crate_name} handlers directory readable: {err}"));
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
                found.insert(format!("{crate_name}/{name}"));
            }
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

#[test]
fn migrated_handlers_use_services_not_the_engine_repository_facade() {
    let workspace = workspace_root();
    for (key, service) in SERVICE_BACKED_HANDLERS {
        let (crate_name, file_name) = key
            .split_once('/')
            .expect("service-backed handler key has crate/file form");
        let path = workspace
            .join(crate_name)
            .join("src/handlers")
            .join(file_name);
        let source =
            fs::read_to_string(&path).unwrap_or_else(|err| panic!("{key} source readable: {err}"));
        assert!(
            source.contains(service),
            "{key} must keep using its {service} application-service seam"
        );
        assert!(
            !source.contains("repository::")
                && !source.contains("runinator_engine::repository")
                && !source.contains("runinator_engine::artifact_storage")
                && !source.contains("runinator_engine::simulate"),
            "{key} bypasses {service} through a runinator-engine implementation module; move that operation into the service instead"
        );
    }
}

#[test]
fn handlers_do_not_call_engine_implementation_modules() {
    let workspace = workspace_root();
    for crate_name in HANDLER_CRATES {
        let dir = workspace.join(crate_name).join("src").join("handlers");
        for entry in fs::read_dir(&dir).expect("handler directory readable") {
            let path = entry.expect("handler entry readable").path();
            if path.extension().and_then(|ext| ext.to_str()) != Some("rs") {
                continue;
            }
            let source = fs::read_to_string(&path).expect("handler source readable");
            assert!(
                !source.contains("repository::")
                    && !source.contains("runinator_engine::repository")
                    && !source.contains("runinator_engine::artifact_storage")
                    && !source.contains("runinator_engine::simulate"),
                "{} calls a runinator-engine implementation module directly; use an injected application service instead",
                path.display()
            );
        }
    }
}
