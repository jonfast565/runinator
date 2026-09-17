#[allow(unused_imports)]
use super::*;

/// a binding from a loop/map variable to the node output it reads from.
#[derive(Clone)]
pub(super) struct VarBinding {
    pub(super) name: String,
    pub(super) node_id: String,
    pub(super) base: Vec<PathSeg>,
}
