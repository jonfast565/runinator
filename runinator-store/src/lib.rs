//! the persistence contract shared by everything that reads or writes runinator state.
//!
//! this crate holds only trait definitions and the plain types they exchange. it has no sqlx
//! dependency and no backend, so a caller can depend on the operations it needs without compiling a
//! database driver, and a test can implement the traits in memory.
//!
//! the surface is split two ways:
//!
//! - [`roles`] carries one trait per persistence domain (runs, schedules, auth, orgs, …).
//!   [`DatabaseImpl`] composes all of them and stays the bound for callers that touch many domains.
//! - [`ReducerStore`] is a *use-case* trait, cut to exactly what the workflow state machine calls.
//!   it deliberately spans several domains; keeping it small is what makes an in-memory fake
//!   practical, and it is why `runinator-reducer` needs no database driver.
//!
//! `runinator-database` provides the concrete sqlite/postgres/mysql implementation and re-exports
//! these modules at their historical paths.

pub mod archive;
pub mod interfaces;
pub mod reducer_store;
pub mod roles;

pub use interfaces::DatabaseImpl;
pub use reducer_store::ReducerStore;

/// every store trait in one import.
///
/// splitting the surface into roles means a caller using operations from several domains has to
/// bring each trait into scope. glob this when that list would be long and uninformative; name the
/// individual traits when a module genuinely only touches one or two.
pub mod prelude {
    pub use crate::interfaces::DatabaseImpl;
    pub use crate::reducer_store::ReducerStore;
    pub use crate::roles::{
        ArchiveStore, AuthStore, AutomationStore, DefinitionStore, DispatchStore,
        NotificationStore, OrgStore, ReplicaStore, RunStore, ScheduleStore, SettingStore,
        TaskRunStore,
    };
}
