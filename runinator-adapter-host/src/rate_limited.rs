#[allow(unused_imports)]
use super::*;

pub(super) struct RateLimited {
    pub(super) retry_after_seconds: Option<u64>,
}
