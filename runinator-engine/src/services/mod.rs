//! application services that coordinate durable work for transport adapters.
//!
//! Each service is bound to the smallest store role it needs. HTTP handlers translate requests and
//! responses around these services; they do not reach through to the repository facade directly.

mod automation_operations;
mod catalog_operations;
mod console_operations;
mod debug_operations;
mod function_invocations;
mod function_packages;
mod ingress_operations;
mod notification_operations;
mod pack_operations;
mod pipeline_operations;
mod replica_registry;
mod run_operations;
mod scheduling_operations;
mod workflow_authoring;
mod workflow_files;

pub use automation_operations::AutomationOperations;
pub use catalog_operations::{CatalogOperations, provider_catalog_item};
pub use console_operations::ConsoleOperations;
pub use debug_operations::DebugOperations;
pub use function_invocations::FunctionInvocations;
pub use function_packages::FunctionPackages;
pub use ingress_operations::IngressOperations;
pub use notification_operations::NotificationOperations;
pub use pack_operations::PackOperations;
pub use pipeline_operations::PipelineOperations;
pub use replica_registry::{
    DEFAULT_REPLICA_DELETE_SECONDS, DEFAULT_REPLICA_REAP_SECONDS, REPLICA_SAMPLE_RETENTION_SECONDS,
    REPLICA_STALE_SECONDS, ReplicaRegistry,
};
pub use run_operations::RunOperations;
pub use scheduling_operations::SchedulingOperations;
pub use workflow_authoring::WorkflowAuthoring;
pub use workflow_files::WorkflowFiles;

#[cfg(test)]
#[path = "pack_operations_tests.rs"]
mod pack_operations_tests;
#[cfg(test)]
#[path = "pipeline_operations_tests.rs"]
mod pipeline_operations_tests;
#[cfg(test)]
#[path = "replica_registry_tests.rs"]
mod replica_registry_tests;
#[cfg(test)]
#[path = "run_operations_tests.rs"]
mod run_operations_tests;
#[cfg(test)]
#[path = "workflow_authoring_tests.rs"]
mod workflow_authoring_tests;
