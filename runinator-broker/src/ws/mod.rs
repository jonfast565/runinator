//! The `WS` broker transport is a third wire transport beside `TCP` and HTTP.
//! It lets a client that cannot reach the broker's private network connect through the public,
//! authenticated `runinator-ws` surface. The client keeps one long-lived, two-way connection.

pub mod client;
#[cfg(feature = "ws")]
mod reconnect;
#[cfg(feature = "ws")]
pub mod server;
pub mod types;

pub use client::WsBroker;
