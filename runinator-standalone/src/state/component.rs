#[allow(unused_imports)]
use super::*;

pub(super) struct Component {
    pub(super) kind: String,
    pub(super) status: String,
    pub(super) restarts: u32,
    pub(super) started: Option<Instant>,
    pub(super) last_error: Option<String>,
}
