//! the web service: it assembles the http surface, applies the middleware stack, serves the
//! WebSocket and openapi endpoints, and hosts the runtime that ties them to the engine.
//!
//! the surface itself is split across sibling crates so no one crate owns both the wire vocabulary
//! and the endpoints written against it:
//!
//! - `runinator-ws-core` — wire payloads, the json response envelope, the ui event bus, the openapi
//!   documentation vocabulary.
//! - `runinator-ws-middleware` — auth, authorization, rate limiting, overload protection.
//! - `runinator-ws-identity` / `-authoring` / `-runtime` — the handler modules, each owning its
//!   `routes()` registrations and its `DOCS` entries.
//!
//! what stays here is assembly: [`router::build_router`] merges every domain's routes,
//! [`openapi`] concatenates every domain's docs, and [`server::run_webserver`] wires the whole thing
//! to the database, broker, and engine.

pub mod errors;
mod event_consumer;
mod openapi;
mod provisioner_config;
pub mod orchestration {
    pub use runinator_reducer::{ReadyNodeDisposition, process_ready_node};
}
mod router;
mod server;
mod websocket;

// the durable orchestration engine (persistence layer, background loops, result consumer) lives in
// runinator-engine and is shared with the standalone background worker. these aliases keep the
// in-crate `crate::repository`/`crate::stability`/… paths pointing at the engine after the extraction.
pub(crate) use runinator_engine::{repository, stability};

// the http surface moved out to the three domain crates; these aliases keep the
// `crate::handlers::<domain>` paths that the openapi `paths(...)` table and the behavior tests use,
// and record which crate owns each domain.
pub(crate) mod handlers {
    pub(crate) use runinator_ws_authoring::handlers::{
        catalog, credentials, packs, pipelines, providers, wdl, workflows,
    };
    pub(crate) use runinator_ws_identity::handlers::{auth, billing, orgs};
    pub(crate) use runinator_ws_runtime::handlers::{
        action_dispatches, agents, artifacts, automation, catalog_metadata, debug, health,
        node_runs, notifications, observability, provisioning, replicas, runs, schedules,
        supervisor, triggers, webhook,
    };
}

// likewise for the shared foundation and the middleware layers.
#[cfg(test)]
pub(crate) use runinator_ws_core::responses;
pub(crate) use runinator_ws_core::{events, models};
pub(crate) use runinator_ws_middleware::{auth, overload, rate_limit};

// the result-consumer loop is re-exported at the engine root; the in-process engine drives it, so
// only the tests reach for it directly under the module path they already use.
#[cfg(test)]
pub(crate) mod result_consumer {
    pub use runinator_engine::{
        ResultConsumerPolicy, run_result_consumer, run_result_consumer_with_policy,
    };
}

pub use router::build_router;
pub use runinator_ws_core::{AppEvent, AppEventKind, EventSender};
pub use runinator_ws_middleware::{AuthOptions, OverloadConfig, RateLimitConfig};
pub use server::{ReplicaAdvertisement, run_webserver};

#[cfg(test)]
pub(crate) use runinator_ws_authoring::handlers::providers::{
    provider_catalog_item, provider_metadata_from_items,
};

/// the sibling crates that own handler modules, as workspace-relative directory names.
///
/// the two source lints that police the http surface — `openapi::route_parity` (is every registered
/// route documented?) and `store_access_tests` (which handlers may touch the store directly?) — both
/// have to read every handler file, and neither can do that from inside one crate anymore. this is
/// the single list they share, so adding a domain crate cannot silently drop it from either guard.
#[cfg(test)]
pub(crate) const HANDLER_CRATES: &[&str] = &[
    "runinator-ws-identity",
    "runinator-ws-authoring",
    "runinator-ws-runtime",
];

/// the workspace root, used by those lints to reach a sibling crate's sources by path.
#[cfg(test)]
pub(crate) fn workspace_root() -> &'static std::path::Path {
    std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("crate sits in the workspace root")
}

#[cfg(test)]
mod store_access_tests;
#[cfg(test)]
mod tests;
