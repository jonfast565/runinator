#[allow(unused_imports)]
use super::*;

pub(crate) struct CallSig {
    pub params: Vec<ParamSig>,
    pub is_user: bool,
    pub effectful: bool,
}

impl CallSig {
    pub(super) fn required_count(&self) -> usize {
        self.params.iter().filter(|p| !p.optional).count()
    }
}
