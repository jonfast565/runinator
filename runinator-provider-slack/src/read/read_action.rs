#[allow(unused_imports)]
use super::*;

pub(crate) struct ReadAction {
    pub function: &'static str,
    pub summary: &'static str,
    pub endpoint: &'static str,
    pub params: &'static [ReadParam],
    // the principal collection/object key in the response, advertised as a result.
    pub result_key: &'static str,
    pub result_is_array: bool,
}
