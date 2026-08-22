//! the foundation every http handler crate writes against: wire payloads, the json error/response
//! envelope, the UI event bus, the openapi documentation vocabulary, and small json helpers.
//!
//! it holds no routes and no middleware. `runinator-ws-middleware` layers request gating on top of
//! it, the `runinator-ws-{identity,authoring,runtime}` crates build handlers on both, and
//! `runinator-ws` merges the result into a served router.

pub mod events;
pub mod json;
pub mod models;
pub mod openapi;
pub mod responses;

pub use events::{AppEvent, AppEventKind, EventBus, EventSender};
pub use json::merge_json;
pub use models::{ApiError, ApiResponse};
