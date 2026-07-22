mod auth;
mod authz;
mod config;
pub mod errors;
mod event_consumer;
mod events;
mod handlers;
mod models;
mod openapi;
mod overload;
mod provisioner_config;
pub mod orchestration {
    pub use runinator_reducer::{ReadyNodeDisposition, process_ready_node};
}
mod rate_limit;
mod responses;
mod router;
mod server;
#[cfg(test)]
mod tests;
mod websocket;

// the durable orchestration engine (persistence layer, background loops, result consumer) lives in
// runinator-engine and is shared with the standalone background worker. these aliases keep the
// in-crate `crate::repository`/`crate::audit`/… paths pointing at the engine after the extraction.
pub(crate) use runinator_engine::{audit, repository, settings, stability};

// the result-consumer loop is re-exported at the engine root; the in-process engine drives it, so
// only the tests reach for it directly under the module path they already use.
#[cfg(test)]
pub(crate) mod result_consumer {
    pub use runinator_engine::{
        ResultConsumerPolicy, run_result_consumer, run_result_consumer_with_policy,
    };
}

pub use auth::AuthOptions;
pub use events::{AppEvent, AppEventKind, EventSender};
pub use overload::OverloadConfig;
pub use rate_limit::RateLimitConfig;
pub use router::build_router;
pub use server::{ReplicaAdvertisement, run_webserver};

#[cfg(test)]
pub(crate) use handlers::providers::{provider_catalog_item, provider_metadata_from_items};
