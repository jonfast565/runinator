//! the agent's patience with an unreachable service or broker, as one counter.
//!
//! an agent loses its connection two different ways, and only one of them restarts the worker loop.
//! a broker that fails to build, or a loop that exits with an error, comes back through
//! [`crate::agent::supervisor::run_supervised`]'s restart path. but a transport that reconnects
//! internally — the WS relay every desktop agent uses by default — drops and re-dials underneath a
//! loop that never notices, because `receive_*` retries across transient reconnects by design. a
//! budget charged only on loop restarts would therefore never fire for the desktop's most common
//! outage, which is precisely "the web service went away".
//!
//! so both axes charge the same budget, and either one can spend it. the count is *consecutive*: a
//! successful connection clears it, so this bounds one unreachable episode rather than a machine's
//! lifetime.

use std::sync::Mutex;
use std::sync::atomic::{AtomicU32, Ordering};

use tokio::sync::watch;

/// consecutive-failure budget shared by the supervisor and the broker connection monitor.

#[cfg(test)]
#[path = "reconnect_tests.rs"]
mod tests;

mod charge;
pub use charge::Charge;

mod reconnect_budget;
pub use reconnect_budget::ReconnectBudget;
