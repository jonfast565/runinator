//! the durable orchestration engine shared by the web service and the standalone background worker.
//!
//! owns the persistence-orchestration layer ([`repository`]) and the background loops that drive the
//! reducer, consume worker results, publish wakes/actions, and run maintenance backstops. the web
//! service embeds this in-process (behind a flag) and `runinator-background-worker` runs it as a
//! separate, horizontally-scalable process; both call [`run_background_engine`].

pub mod audit;
pub mod errors;
pub mod events;
pub mod repository;
pub mod repository_runs;
pub mod repository_state;
pub mod settings;
pub mod simulate;
pub mod stability;

mod engine;
mod loops;
mod result_consumer;

// re-export the reducer under the `orchestration` path the repository layer references.
pub mod orchestration {
    pub use runinator_reducer::{ReadyNodeDisposition, process_ready_node};
}

pub use engine::run_background_engine;
pub use events::{AppEvent, EnginePublisher, EventSender};

// exposed so the web service can reuse the same result-consumer policy/loop in-process.
pub use result_consumer::{
    ResultConsumerPolicy, run_result_consumer, run_result_consumer_with_policy,
};
