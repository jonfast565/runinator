//! Portable workspace contents and immutable snapshot references.

use super::*;
use std::collections::BTreeMap;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WorkspaceAccess {
    Read,
    #[default]
    Write,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum WorkspaceOrigin {
    Workflow {
        workflow_run_id: Uuid,
        effect_id: Uuid,
        attempt: u32,
    },
    Import {
        transfer_id: Uuid,
        format: String,
    },
}
impl WorkspaceOrigin {
    pub fn effect_id(&self) -> Option<Uuid> {
        match self {
            Self::Workflow { effect_id, .. } => Some(*effect_id),
            Self::Import { .. } => None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "state", rename_all = "snake_case")]
pub enum WorkspaceAcquisition {
    Acquired { checkout: WorkspaceCheckout },
    Busy,
    Conflict,
    Missing,
}

#[path = "workspace_contents/workspace_reference.rs"]
mod workspace_reference;
pub use workspace_reference::WorkspaceReference;

#[path = "workspace_contents/workspace_attachment.rs"]
mod workspace_attachment;
pub use workspace_attachment::WorkspaceAttachment;

#[path = "workspace_contents/durable_workspace.rs"]
mod durable_workspace;
pub use durable_workspace::DurableWorkspace;

#[path = "workspace_contents/workspace_snapshot.rs"]
mod workspace_snapshot;
pub use workspace_snapshot::WorkspaceSnapshot;

#[path = "workspace_contents/workspace_checkout.rs"]
mod workspace_checkout;
pub use workspace_checkout::WorkspaceCheckout;

#[path = "workspace_contents/workspace_commit.rs"]
mod workspace_commit;
pub use workspace_commit::WorkspaceCommit;

#[path = "workspace_contents/workspace_object_location.rs"]
mod workspace_object_location;
pub use workspace_object_location::WorkspaceObjectLocation;

#[path = "workspace_contents/workspace_seal.rs"]
mod workspace_seal;
pub use workspace_seal::WorkspaceSeal;

#[path = "workspace_contents/workspace_receipt.rs"]
mod workspace_receipt;
pub use workspace_receipt::WorkspaceReceipt;

#[path = "workspace_contents/workspace_entry.rs"]
mod workspace_entry;
pub use workspace_entry::WorkspaceEntry;

#[path = "workspace_contents/workspace_directory.rs"]
mod workspace_directory;
pub use workspace_directory::WorkspaceDirectory;

#[path = "workspace_contents/workspace_download.rs"]
mod workspace_download;
pub use workspace_download::WorkspaceDownload;

#[path = "workspace_contents/workspace_download_request.rs"]
mod workspace_download_request;
pub use workspace_download_request::WorkspaceDownloadRequest;

#[path = "workspace_contents/workspace_difference.rs"]
mod workspace_difference;
pub use workspace_difference::WorkspaceDifference;

#[path = "workspace_contents/workspace_diff.rs"]
mod workspace_diff;
pub use workspace_diff::WorkspaceDiff;

#[path = "workspace_contents/workspace_gc_lease.rs"]
mod workspace_gc_lease;
pub use workspace_gc_lease::WorkspaceGcLease;

#[path = "workspace_contents/workspace_reader_lease.rs"]
mod workspace_reader_lease;
pub use workspace_reader_lease::WorkspaceReaderLease;

#[path = "workspace_contents/workspace_acquire.rs"]
mod workspace_acquire;
pub use workspace_acquire::WorkspaceAcquire;

#[path = "workspace_contents/workspace_execution.rs"]
mod workspace_execution;
pub use workspace_execution::WorkspaceExecution;

#[path = "workspace_contents/workspace_view.rs"]
mod workspace_view;
pub use workspace_view::WorkspaceView;

#[path = "workspace_contents/workspace_transfer.rs"]
mod workspace_transfer;
pub use workspace_transfer::WorkspaceTransfer;
