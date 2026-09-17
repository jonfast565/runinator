//! the single seam between the shared agent lifecycle and whatever is hosting it. the headless
//! binary leaves every hook at its default (tracing inside the loops already covers it); a gui host
//! implements them to drive a status header, a console, and native notifications.

use crate::agent::status::AgentStatus;
use crate::events::WorkerEvent;

/// host hooks for agent lifecycle activity. implementations must be cheap and non-blocking: hooks
/// are called inline from the lifecycle task and from the worker loops.

/// default observer that ignores everything; the headless default.
mod agent_observer;
pub use agent_observer::AgentObserver;

mod noop_observer;
pub use noop_observer::NoopObserver;
