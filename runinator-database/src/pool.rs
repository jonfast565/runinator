//! connection-pool sizing shared by every sql backend. bounding the pool caps how many server
//! connections a request flood can open, and an acquisition timeout turns a saturated pool into a
//! fast error instead of an unbounded wait that ties up an http worker. both are env-tunable so the
//! defaults stay sane for the local stack while production can raise them to match the database.

use std::time::Duration;

/// default maximum pooled connections. sqlx's built-in default is 10; 20 gives the web service and
/// in-process engine headroom without risking a small managed postgres's connection cap.
const DEFAULT_MAX_CONNECTIONS: u32 = 20;

/// default acquire timeout. long enough to ride out a brief burst, short enough that a genuinely
/// saturated pool fails fast rather than parking the caller indefinitely (sqlx's default is 30s).
const DEFAULT_ACQUIRE_TIMEOUT_SECONDS: u64 = 30;

/// maximum pooled connections, overridable via `RUNINATOR_DB_MAX_CONNECTIONS`. a missing, unparseable,
/// or zero value falls back to the default.
pub(crate) fn pool_max_connections() -> u32 {
    std::env::var("RUNINATOR_DB_MAX_CONNECTIONS")
        .ok()
        .and_then(|raw| raw.trim().parse::<u32>().ok())
        .filter(|value| *value > 0)
        .unwrap_or(DEFAULT_MAX_CONNECTIONS)
}

/// pool acquisition timeout, overridable via `RUNINATOR_DB_ACQUIRE_TIMEOUT_SECONDS`. a missing,
/// unparseable, or zero value falls back to the default.
pub(crate) fn pool_acquire_timeout() -> Duration {
    let seconds = std::env::var("RUNINATOR_DB_ACQUIRE_TIMEOUT_SECONDS")
        .ok()
        .and_then(|raw| raw.trim().parse::<u64>().ok())
        .filter(|value| *value > 0)
        .unwrap_or(DEFAULT_ACQUIRE_TIMEOUT_SECONDS);
    Duration::from_secs(seconds)
}
