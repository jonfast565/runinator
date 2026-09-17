#[allow(unused_imports)]
use super::*;

#[derive(Default)]
pub(super) struct DeltaPlan {
    pub(super) whiteouts: Vec<String>,
    pub(super) emit: Vec<String>,
}
