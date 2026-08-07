//! the authoring http surface: workflow definitions and revisions, the WDL language endpoints,
//! compiled pack import, pipelines, the settings/secrets store, the authoring catalog, and provider
//! registration.
//!
//! everything here describes what *can* run. the endpoints that drive things that *are* running live
//! in `runinator-ws-runtime`.

pub mod handlers;

// persistence orchestration and settings encoding live in the engine; these aliases keep the
// `crate::repository`/`crate::settings` paths the moved handler code already uses.
pub(crate) use runinator_engine::{repository, settings};
