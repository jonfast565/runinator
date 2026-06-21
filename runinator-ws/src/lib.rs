mod audit;
mod auth;
mod authz;
mod background;
mod config;
pub mod errors;
mod events;
mod handlers;
mod models;
mod openapi;
pub mod orchestration {
    pub use runinator_reducer::{ReadyNodeDisposition, process_ready_node};
}
mod rate_limit;
mod repository;
mod repository_runs;
mod repository_state;
mod responses;
mod result_consumer;
mod router;
mod server;
mod settings;
mod stability;
#[cfg(test)]
mod tests;
mod websocket;

pub use auth::AuthOptions;
pub use events::{AppEvent, EventSender};
pub use rate_limit::RateLimitConfig;
pub use router::build_router;
pub use server::{ReplicaAdvertisement, run_webserver};

#[cfg(test)]
pub(crate) use handlers::providers::{provider_catalog_item, provider_metadata_from_items};
