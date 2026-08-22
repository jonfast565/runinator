//! the authoring http surface: workflow definitions and revisions, the REXRAP language endpoints,
//! compiled pack import, pipelines, the settings/secrets store, the authoring catalog, and provider
//! registration.
//!
//! everything here describes what *can* run. the endpoints that drive things that *are* running live
//! in `runinator-ws-runtime`.

pub mod handlers;

// settings encoding lives in the engine. Handler modules name the application boundary they use
// directly so this crate does not grow another repository facade.
pub(crate) use runinator_engine::settings;
