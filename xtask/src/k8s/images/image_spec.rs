#[allow(unused_imports)]
use super::*;

pub(super) struct ImageSpec {
    pub(super) name: &'static str,
    pub(super) dockerfile: &'static str,
    pub(super) target: Option<&'static str>,
    pub(super) context: &'static str,
}
