#[allow(unused_imports)]
use super::*;

pub(super) struct Node {
    pub(super) kind: ReplicaKind,
    pub(super) group: String,
    pub(super) spec: NodeSpec,
    pub(super) shutdown: Arc<Notify>,
    pub(super) task: JoinHandle<()>,
}
