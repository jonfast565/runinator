use std::collections::BTreeMap;
use std::path::PathBuf;

use async_trait::async_trait;
use runinator_models::errors::SendableError;
use runinator_models::provisioning::{NodeSpec, ProvisionBackend, ProvisionedGroup};
use runinator_models::replicas::ReplicaKind;
use runinator_supervisor::config::ProcessConfig;
use runinator_supervisor::control::{enqueue, ControlCommand};
use runinator_supervisor::snapshot::read_snapshot;
use uuid::Uuid;

use crate::errors::{ENQUEUE_FAILED, SNAPSHOT_READ, UNSUPPORTED_KIND};
use crate::traits::Provisioner;

// statuses the supervisor reports for a process that is up (or coming up).
const LIVE_STATUSES: &[&str] = &["running", "starting"];

/// provisions worker/waker nodes as dynamic supervisor processes.

// the effective group for a scale request: an explicit `spec.group` (e.g. a per-org pool) or the
// kind's default group. the kind default keeps pre-group behavior byte-compatible.
fn effective_group(kind: ReplicaKind, spec: &NodeSpec) -> String {
    spec.group
        .clone()
        .unwrap_or_else(|| kind.as_str().to_string())
}

// the process-name prefix that marks processes as provisioner-managed for a group.
fn prefix_for(group: &str) -> String {
    format!("prov-{group}-")
}

// The CLI flag a node binary uses to receive its generated ID.
fn id_flag(kind: ReplicaKind) -> &'static str {
    match kind {
        ReplicaKind::Worker => "--worker-id",
        ReplicaKind::Waker => "--waker-id",
        ReplicaKind::Webservice
        | ReplicaKind::Background
        | ReplicaKind::Postgres
        | ReplicaKind::Archiver => "--instance-id",
    }
}

mod supervisor_node_template;
pub use supervisor_node_template::SupervisorNodeTemplate;

mod supervisor_provisioner;
pub use supervisor_provisioner::SupervisorProvisioner;
