#[allow(unused_imports)]
use super::*;

pub(super) struct TriggerRunContext<'a> {
    pub(super) scheduler_id: &'a str,
    pub(super) slot: DateTime<Utc>,
    pub(super) now: DateTime<Utc>,
}
