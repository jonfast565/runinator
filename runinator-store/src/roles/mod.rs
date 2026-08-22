//! the role traits `DatabaseImpl` is composed from.
//!
//! the store's operations split by domain so a caller can bound on the slice it uses instead of
//! the whole 200-plus-method surface. `RuntimeStore` (one level up) is the exception: it is a
//! use-case trait cut to what the state machine calls, deliberately spanning several of these
//! domains, because keeping it small is what makes an in-memory fake practical.

use chrono::{DateTime, Utc};

/// A bounded operational view of one durable queue. The metrics layer converts the timestamp to an
/// age at observation time; keeping the timestamp here avoids baking clock policy into storage.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct QueueSnapshot {
    pub depth: u64,
    pub claimed: u64,
    pub oldest_enqueued_at: Option<DateTime<Utc>>,
}

pub mod archive;
pub mod auth;
pub mod automation;
pub mod console;
pub mod definitions;
pub mod delivery;
pub mod functions;
pub mod notifications;
pub mod orgs;
pub mod rbac;
pub mod replicas;
pub mod runs;
pub mod schedules;
pub mod settings;
pub mod task_runs;
pub mod workflow_vm;

pub use archive::ArchiveStore;
pub use auth::AuthStore;
pub use automation::AutomationStore;
pub use console::ConsoleStore;
pub use definitions::DefinitionStore;
pub use delivery::DeliveryStore;
pub use functions::FunctionStore;
pub use notifications::NotificationStore;
pub use orgs::OrgStore;
pub use rbac::RbacStore;
pub use replicas::ReplicaStore;
pub use runs::RunStore;
pub use schedules::{ScheduleStore, ScheduledWorkflowVm};
pub use settings::SettingStore;
pub use task_runs::TaskRunStore;
pub use workflow_vm::{NewWorkflowVmRun, WorkflowVmStore};
