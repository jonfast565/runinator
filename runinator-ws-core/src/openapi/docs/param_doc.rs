#[allow(unused_imports)]
use super::*;

#[derive(Clone, Copy)]
pub struct ParamDoc {
    pub name: &'static str,
    pub location: &'static str,
    pub description: &'static str,
    pub required: bool,
    pub example: &'static str,
}
