mod errors;
mod params;
mod provider;
mod runner;

pub use provider::ConsoleProvider;

/// env var a worker sets to declare it runs in an interactive desktop session, permitting
/// `console.run(interactive: true)`. the desktop agent sets it; a headless cloud worker does not, so
/// an interactive console command routed there is rejected instead of hanging with no terminal.
pub const ALLOW_INTERACTIVE_ENV: &str = "RUNINATOR_CONSOLE_ALLOW_INTERACTIVE";

/// env var a worker sets to the base directory console commands run from (the child's `current_dir`).
/// lets a command reference files with a relative path (`bash scripts/sync-secrets.sh`) instead of an
/// absolute one baked in at import. unset/empty means inherit the worker process's cwd, as before. the
/// desktop agent sets it from its configured working directory.
pub const WORKING_DIR_ENV: &str = "RUNINATOR_CONSOLE_WORKING_DIR";

#[cfg(test)]
mod tests;
