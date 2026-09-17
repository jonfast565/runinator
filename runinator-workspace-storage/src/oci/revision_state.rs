#[allow(unused_imports)]
use super::*;

#[derive(Clone)]
pub(super) struct RevisionState {
    pub(super) workspace: Workspace,
    pub(super) projection: PathProjection,
}
