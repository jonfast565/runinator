#[allow(unused_imports)]
use super::*;

/// supervisor-backend configuration: where to read state / enqueue control, plus a spawn template
/// per node kind. keying by kind means a new kind is manageable as soon as a template is added.
#[derive(Debug, Clone)]
pub struct SupervisorBackendConfig {
    pub control_dir: PathBuf,
    pub state_file: PathBuf,
    pub templates: BTreeMap<ReplicaKind, SupervisorNodeTemplate>,
}
