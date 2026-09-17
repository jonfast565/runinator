#[allow(unused_imports)]
use super::*;

#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) struct StyledSpan {
    pub(super) text: String,
    pub(super) style: Style,
}
