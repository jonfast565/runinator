#[allow(unused_imports)]
use super::*;

#[derive(Default)]
pub(super) struct MetricHistory {
    pub(super) healthy_percent: VecDeque<u64>,
    pub(super) restart_events: VecDeque<u64>,
    pub(super) previous_restarts: BTreeMap<String, u32>,
}

impl MetricHistory {
    pub(super) fn observe(&mut self, snapshot: &StateSnapshot) {
        let total = snapshot.processes.len();
        let healthy = snapshot
            .processes
            .iter()
            .filter(|process| status_tone(&process.status) == StatusTone::Good)
            .count();
        let percent = healthy
            .saturating_mul(100)
            .checked_div(total)
            .unwrap_or_default() as u64;
        push_sample(&mut self.healthy_percent, percent);

        let mut restarts = 0;
        let mut current = BTreeMap::new();
        for process in &snapshot.processes {
            let previous = self
                .previous_restarts
                .get(&process.name)
                .copied()
                .unwrap_or(process.restarts);
            restarts += u64::from(process.restarts.saturating_sub(previous));
            current.insert(process.name.clone(), process.restarts);
        }
        self.previous_restarts = current;
        push_sample(&mut self.restart_events, restarts);
    }

    pub(super) fn restart_count(&self) -> u64 {
        self.restart_events.iter().sum()
    }
}
