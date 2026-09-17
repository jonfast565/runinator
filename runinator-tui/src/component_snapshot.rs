#[allow(unused_imports)]
use super::*;

pub(super) struct ComponentSnapshot {
    pub(super) name: &'static str,
    pub(super) details: Vec<String>,
    pub(super) activity: String,
    pub(super) activity_age: Duration,
    pub(super) expected_remaining: Option<Duration>,
    pub(super) counters: BTreeMap<&'static str, u64>,
    pub(super) gauges: BTreeMap<&'static str, i64>,
}
