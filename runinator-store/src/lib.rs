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
//! - [`RuntimeStore`] is a *use-case* trait, cut to exactly what the workflow state machine calls.
//!   it deliberately spans several domains; keeping it small is what makes an in-memory fake
//!   practical, and it is why `runinator-runtime` needs no database driver.
//!
//! `runinator-database` provides the concrete sqlite/postgres/mysql implementation.

use std::future::Future;

use runinator_models::errors::SendableError;

pub mod archive;
pub mod pack_transaction;
pub mod roles;
pub mod runtime_store;
pub mod workflow_mutex;

pub use pack_transaction::PackTransactionStore;
pub use runtime_store::RuntimeStore;

/// The full persistence surface: every role trait, composed.
///
/// This stays the bound for composition roots and genuinely whole-store work such as schema
/// initialization. A caller that needs one slice should bound on that role (or a small named
/// use-case contract) instead, the way `runinator-runtime` bounds on `RuntimeStore`.
///
/// Because the roles are separate traits, calling methods from several of them means bringing each
/// into scope; glob [`prelude`] when that list would be long and uninformative.
pub trait DatabaseImpl:
    RuntimeStore
    + PackTransactionStore
    + roles::ArchiveStore
    + roles::DefinitionStore
    + roles::DeliveryStore
    + roles::ScheduleStore
    + roles::RunStore
    + roles::ConsoleStore
    + roles::FunctionStore
    + roles::FileStore
    + roles::AutomationStore
    + roles::NotificationStore
    + roles::ReplicaStore
    + roles::SettingStore
    + roles::AuthStore
    + roles::RbacStore
    + roles::OrgStore
    + roles::WorkflowVmStore
{
    /// Execute initialization scripts for the database.
    ///
    /// The one operation that is not a domain operation: it brings the schema up, so it stays on
    /// the composed trait rather than in any single role.
    fn run_init_scripts(
        &self,
        paths: &[String],
    ) -> impl Future<Output = Result<(), SendableError>> + Send;
}

/// every store trait in one import.
///
/// splitting the surface into roles means a caller using operations from several domains has to
/// bring each trait into scope. glob this when that list would be long and uninformative; name the
/// individual traits when a module genuinely only touches one or two.
pub mod prelude {
    pub use crate::DatabaseImpl;
    pub use crate::pack_transaction::PackTransactionStore;
    pub use crate::roles::{
        ArchiveStore, AuthStore, AutomationStore, ConsoleStore, DefinitionStore, DeliveryStore,
        FileStore, FunctionStore, NewWorkflowVmRun, NotificationStore, OrgStore, QueueSnapshot,
        RbacStore, ReplicaStore, RunStore, ScheduleStore, ScheduledWorkflowVm, SettingStore,
        WorkflowVmStore,
    };
    pub use crate::runtime_store::RuntimeStore;
}
