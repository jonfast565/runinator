//! local-files provider: sandboxed read/write/list/stat/delete on the worker's own machine, for an
//! embedded desktop worker. kept out of the shared server catalog so cloud workers never touch a
//! user's disk; results are tagged `location: "local"` to distinguish them from cloud artifacts.

mod errors;
mod params;
mod provider;
mod runner;
mod sandbox;

pub use provider::LocalProvider;

#[cfg(test)]
mod tests;
