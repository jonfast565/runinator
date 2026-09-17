#[allow(unused_imports)]
use super::*;

pub struct Dashboard {
    pub(super) started: Instant,
    pub(super) components: RwLock<BTreeMap<&'static str, Component>>,
    pub(super) logs: RwLock<VecDeque<StyledLine>>,
    pub(super) ansi: Mutex<AnsiParser>,
}

impl Default for Dashboard {
    fn default() -> Self {
        Self {
            started: Instant::now(),
            components: RwLock::new(BTreeMap::new()),
            logs: RwLock::new(VecDeque::with_capacity(MAX_LOG_LINES)),
            ansi: Mutex::new(AnsiParser::default()),
        }
    }
}

impl Dashboard {
    pub(super) fn register(&self, name: &'static str, details: impl IntoIterator<Item = String>) {
        let mut components = self
            .components
            .write()
            .unwrap_or_else(|err| err.into_inner());
        let component = components.entry(name).or_default();
        component.details = details.into_iter().collect();
    }

    pub(super) fn activity(&self, name: &'static str, what: String, expected: Option<Duration>) {
        let mut components = self
            .components
            .write()
            .unwrap_or_else(|err| err.into_inner());
        let component = components.entry(name).or_default();
        component.activity = what;
        component.activity_started = Instant::now();
        component.expected_done = expected.map(|duration| Instant::now() + duration);
    }

    pub(super) fn counter(&self, name: &'static str, metric: &'static str, amount: u64) {
        let mut components = self
            .components
            .write()
            .unwrap_or_else(|err| err.into_inner());
        let component = components.entry(name).or_default();
        *component.counters.entry(metric).or_default() += amount;
    }

    pub(super) fn gauge(&self, name: &'static str, metric: &'static str, value: i64) {
        let mut components = self
            .components
            .write()
            .unwrap_or_else(|err| err.into_inner());
        let component = components.entry(name).or_default();
        component.gauges.insert(metric, value);
    }

    pub(super) fn gauge_increment(&self, name: &'static str, metric: &'static str, amount: i64) {
        let mut components = self
            .components
            .write()
            .unwrap_or_else(|err| err.into_inner());
        let component = components.entry(name).or_default();
        *component.gauges.entry(metric).or_default() += amount;
    }

    pub(super) fn log_line(&self, line: String) {
        let line = self
            .ansi
            .lock()
            .unwrap_or_else(|err| err.into_inner())
            .parse_line(&line);
        if line.plain.is_empty() {
            return;
        }
        let mut logs = self.logs.write().unwrap_or_else(|err| err.into_inner());
        if logs.len() == MAX_LOG_LINES {
            logs.pop_front();
        }
        logs.push_back(line);
    }

    pub(super) fn snapshot(&self) -> DashboardSnapshot {
        let components = self
            .components
            .read()
            .unwrap_or_else(|err| err.into_inner());
        let logs = self
            .logs
            .read()
            .unwrap_or_else(|err| err.into_inner())
            .iter()
            .cloned()
            .collect();
        DashboardSnapshot {
            uptime: self.started.elapsed(),
            components: components
                .iter()
                .map(|(name, component)| ComponentSnapshot {
                    name,
                    details: component.details.clone(),
                    activity: component.activity.clone(),
                    activity_age: component.activity_started.elapsed(),
                    expected_remaining: component
                        .expected_done
                        .map(|deadline| deadline.saturating_duration_since(Instant::now())),
                    counters: component.counters.clone(),
                    gauges: component.gauges.clone(),
                })
                .collect(),
            logs,
        }
    }
}
