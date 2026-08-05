//! the role traits `DatabaseImpl` is composed from.
//!
//! the store's operations split by domain so a caller can bound on the slice it uses instead of
//! the whole 200-plus-method surface. `ReducerStore` (one level up) is the exception: it is a
//! use-case trait cut to what the state machine calls, deliberately spanning several of these
//! domains, because keeping it small is what makes an in-memory fake practical.

pub mod archive;
pub mod auth;
pub mod automation;
pub mod definitions;
pub mod dispatch;
pub mod notifications;
pub mod orgs;
pub mod replicas;
pub mod runs;
pub mod schedules;
pub mod settings;
pub mod task_runs;

pub use archive::ArchiveStore;
pub use auth::AuthStore;
pub use automation::AutomationStore;
pub use definitions::DefinitionStore;
pub use dispatch::DispatchStore;
pub use notifications::NotificationStore;
pub use orgs::OrgStore;
pub use replicas::ReplicaStore;
pub use runs::RunStore;
pub use schedules::ScheduleStore;
pub use settings::SettingStore;
pub use task_runs::TaskRunStore;
