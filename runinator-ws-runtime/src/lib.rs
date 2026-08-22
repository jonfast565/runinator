//! the runtime http surface: workflow and task runs, VM continuations and effects, artifacts,
//! triggers and schedules, automation records, notifications, the debugger,
//! replicas and node provisioning, webhook ingress, observability, and the health probes.
//!
//! these endpoints drive and observe work that is already running; the endpoints that define what
//! can run live in `runinator-ws-authoring`.

pub mod handlers;

// the panic counter lives in the engine. Handler modules name their application boundary directly
// so this crate does not grow another repository facade.
pub(crate) use runinator_engine::stability;
