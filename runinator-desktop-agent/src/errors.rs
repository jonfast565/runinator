// the dictionary doubles as documentation; entries are reachable only from the headless path, so
// allow unused items in this bin crate.
#![allow(dead_code)]

use runinator_models::errors::{EngineErrors, ErrorDescriptor};

// numbered error dictionary for the desktop agent (RUNI25x, inside the worker crate family's
// RUNI2xx range — the desktop agent is a host for `runinator-worker`'s runtime, not its own engine).

pub const RUNTIME_BUILD: ErrorDescriptor = ErrorDescriptor::new(
    "RUNI251",
    "desktop_agent.runtime",
    "Failed to build the desktop agent runtime",
);
pub const SIGNAL_CTRL_C: ErrorDescriptor = ErrorDescriptor::new(
    "RUNI252",
    "desktop_agent.signal.ctrl_c",
    "Failed to listen for Ctrl+C",
);

pub const DICTIONARY: &[ErrorDescriptor] = &[RUNTIME_BUILD, SIGNAL_CTRL_C];

/// desktop agent error dictionary.
pub struct DesktopAgentErrors;

impl EngineErrors for DesktopAgentErrors {
    fn error_dictionary() -> &'static [ErrorDescriptor] {
        DICTIONARY
    }
}
