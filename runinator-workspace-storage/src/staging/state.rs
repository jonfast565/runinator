#[allow(unused_imports)]
use super::*;

pub(super) struct State {
    pub(super) file: File,
    pub(super) objects: HashMap<Id, StagedObject>,
}
