//! the `ws` broker transport: a third wire transport alongside `tcp`/`http`, built for a client that
//! can't reach the broker's internal network directly (e.g. an operator's desktop-agent talking
//! through `runinator-ws`'s public, authenticated surface instead of straight to RabbitMQ). unlike
//! the other two transports, `client`'s connection is long-lived, bidirectional, and multiplexed —
//! see its module doc for the concurrency model.

pub mod client;
#[cfg(feature = "ws")]
mod reconnect;
#[cfg(feature = "ws")]
pub mod server;
pub mod types;

pub use client::WsBroker;
