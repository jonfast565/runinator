//! the composed persistence surface.
//!
//! the operations themselves live in [`crate::roles`], one trait per domain, plus
//! [`crate::runtime_store::RuntimeStore`] for the state machine's slice. this module only stitches
//! them together and keeps the historical `runinator_database::interfaces::*` import path working.

use std::future::Future;

use runinator_models::errors::SendableError;

// callers reaching for the contract at its historical path find every trait here, either by name or
// through `interfaces::prelude::*`. this also brings the role traits into scope for the supertrait
// list below — a private `use` alongside it would shadow the re-export and make them unreachable.
pub use crate::prelude;
pub use crate::prelude::*;

/// the full persistence surface: every role trait, composed.
///
/// this stays the bound for composition roots and genuinely whole-store work such as schema
/// initialization. a caller that needs one slice should bound on that role (or a small named
/// use-case contract) instead, the way `runinator-runtime` bounds on `RuntimeStore`.
///
/// because the roles are separate traits, calling methods from several of them means bringing each
/// into scope; glob [`prelude`] when that list would be long and uninformative.
pub trait DatabaseImpl:
    RuntimeStore
    + ArchiveStore
    + DefinitionStore
    + DeliveryStore
    + TaskRunStore
    + ScheduleStore
    + RunStore
    + ConsoleStore
    + FunctionStore
    + AutomationStore
    + NotificationStore
    + ReplicaStore
    + SettingStore
    + AuthStore
    + RbacStore
    + OrgStore
    + WorkflowVmStore
{
    /// Execute initialization scripts for the database.
    ///
    /// the one operation that is not a domain operation: it brings the schema up, so it stays on the
    /// composed trait rather than in any single role.
    fn run_init_scripts(
        &self,
        paths: &[String],
    ) -> impl Future<Output = Result<(), SendableError>> + Send;
}
