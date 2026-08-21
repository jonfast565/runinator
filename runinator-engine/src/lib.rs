//! the durable orchestration engine shared by the web service and the standalone engine worker.
//!
//! owns the persistence-orchestration layer ([`repository`]) and the background loops that drive the
//! graph runtime, consume worker results, publish wakes/actions, and run maintenance backstops. the web
//! service embeds this in-process (behind a flag) and `runinator-engine-worker` runs it as a
//! separate, horizontally-scalable process; both call [`run_background_engine`].

pub mod artifact_storage;
pub mod audit;
pub mod errors;
pub mod events;
pub mod notifications;
pub mod repository;
pub mod repository_runs;
pub mod settings;
pub mod simulate;
pub mod stability;

mod effect_consumer;
mod engine;
mod infrastructure_effect_host;
mod loops;

pub use engine::{EngineConfig, run_background_engine};
pub use events::{AppEvent, AppEventKind, EnginePublisher, EventSender};
pub use infrastructure_effect_host::run_infrastructure_effect_host;

pub use effect_consumer::run_effect_result_consumer;
