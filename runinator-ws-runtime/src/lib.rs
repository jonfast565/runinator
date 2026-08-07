//! the runtime http surface: workflow and task runs, node runs, artifacts, triggers and schedules,
//! the action-dispatch outbox, automation records (gates and approvals), notifications, the debugger,
//! replicas and node provisioning, webhook ingress, observability, and the health probes.
//!
//! these endpoints drive and observe work that is already running; the endpoints that define what
//! can run live in `runinator-ws-authoring`.

pub mod handlers;

// persistence orchestration, audit records, and the panic counter live in the engine; these aliases
// keep the `crate::repository`/`crate::audit`/`crate::stability` paths the moved handler code uses.
pub(crate) use runinator_engine::{audit, repository, stability};
