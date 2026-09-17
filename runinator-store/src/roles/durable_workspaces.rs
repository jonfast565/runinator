//! Portable workspace identities, snapshots, and fenced execution checkouts.

use runinator_models::{errors::SendableError, workspaces::*};
use std::future::Future;
use uuid::Uuid;

mod durable_workspace_store;
pub use durable_workspace_store::DurableWorkspaceStore;

mod workspace_retention_store;
pub use workspace_retention_store::WorkspaceRetentionStore;

mod workspace_transfer_store;
pub use workspace_transfer_store::WorkspaceTransferStore;
