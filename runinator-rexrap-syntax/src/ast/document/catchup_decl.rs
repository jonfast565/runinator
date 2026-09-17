#[allow(unused_imports)]
use super::*;

/// a `catchup <policy> [grace <duration>] [max <n>]` option on a header cron trigger. `grace`
/// applies to `skip` (how late a slot may be before it is abandoned) and `max` to `fire_all` (how
/// many missed slots one pass replays).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CatchupDecl {
    pub policy: CatchupPolicy,
    pub grace_seconds: Option<i64>,
    pub max_slots: Option<i64>,
}
