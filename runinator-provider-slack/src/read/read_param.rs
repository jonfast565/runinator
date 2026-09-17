#[allow(unused_imports)]
use super::*;

pub(crate) struct ReadParam {
    pub name: &'static str,
    pub kind: ParamKind,
    pub required: bool,
    pub description: &'static str,
}
