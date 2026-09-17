//! local composition-host status shared by the API, standalone daemon, and command center.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum LocalRuntimeHostKind {
    Standalone,
    Supervisor,
}

mod local_runtime_snapshot;
pub use local_runtime_snapshot::LocalRuntimeSnapshot;

mod local_runtime_component_snapshot;
pub use local_runtime_component_snapshot::LocalRuntimeComponentSnapshot;

mod local_dashboard_snapshot;
pub use local_dashboard_snapshot::LocalDashboardSnapshot;

mod local_resource_sample;
pub use local_resource_sample::LocalResourceSample;
