#[allow(unused_imports)]
use super::*;

#[derive(Clone)]
pub(super) struct Component {
    pub(super) details: Vec<String>,
    pub(super) activity: String,
    pub(super) activity_started: Instant,
    pub(super) expected_done: Option<Instant>,
    pub(super) counters: BTreeMap<&'static str, u64>,
    pub(super) gauges: BTreeMap<&'static str, i64>,
}

impl Default for Component {
    fn default() -> Self {
        Self {
            details: Vec::new(),
            activity: "starting".to_string(),
            activity_started: Instant::now(),
            expected_done: None,
            counters: BTreeMap::new(),
            gauges: BTreeMap::new(),
        }
    }
}
