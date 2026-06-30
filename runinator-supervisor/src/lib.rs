// public surface used by other crates (e.g., runinator-ws supervisor status endpoint and the
// provisioner control queue). the config + control modules are exported so external callers can
// build process templates and enqueue dynamic add/start/stop/remove commands.

pub mod config;
pub mod control;
pub mod snapshot;
pub mod types;

#[cfg(test)]
mod tests;
