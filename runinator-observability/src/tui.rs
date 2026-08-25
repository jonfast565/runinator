//! A small in-process runtime dashboard shared by the local Runinator binaries.
//!
//! This is intentionally separate from OTEL and Prometheus: those transports are for collecting
//! history outside the process, while this surface is a zero-setup answer to "what is this local
//! process doing right now?". Instrumentation remains a no-op unless a binary was started with
//! `--tui` on an interactive terminal.

use std::{
    collections::{BTreeMap, VecDeque},
    io::{self, IsTerminal, Write},
    sync::{Arc, OnceLock, RwLock},
    thread,
    time::{Duration, Instant},
};

use ratatui::{
    Terminal,
    backend::CrosstermBackend,
    crossterm::{
        event::{self, Event, KeyCode, KeyEventKind},
        execute,
        terminal::{EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode},
    },
    layout::{Constraint, Direction, Layout},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Paragraph, Sparkline, Wrap},
};
use tracing_subscriber::fmt::MakeWriter;

use crate::resource_telemetry::TelemetryCollector;

mod capture;

static DASHBOARD: OnceLock<Arc<Dashboard>> = OnceLock::new();

/// The compact dashboard reserves exactly three rows for recent events. Retaining the same number
/// avoids a growing in-memory log and guarantees that the bottom panel never crowds out runtime
/// status on a normal terminal.
const MAX_LOG_LINES: usize = 3;
/// Keep enough points for one minute of once-per-second resource samples. This is intentionally
/// local to the dashboard: durable, cross-replica history remains the job of heartbeats and OTEL.
const RESOURCE_HISTORY_CAPACITY: usize = 60;
const RESOURCE_SAMPLE_INTERVAL: Duration = Duration::from_secs(1);

/// Configure terminal-safe logging and return whether the interactive dashboard can be started.
/// A `--tui` invocation in a pipe or supervisor daemon deliberately falls back to ordinary logs.
pub fn prepare(enabled: bool) -> bool {
    if !enabled {
        return false;
    }
    if !io::stdout().is_terminal() {
        eprintln!("--tui needs an interactive terminal; continuing with normal logging.");
        return false;
    }
    // Safety: this executes before ProcessResources installs the tracing subscriber, and only
    // tells that logger to keep stdout clear for the alternate-screen dashboard.
    unsafe {
        std::env::set_var("RUNINATOR_TUI", "1");
    }
    // Install before the process logger so early startup diagnostics are eligible for the rolling
    // log pane; later callers of `install` receive this same shared state.
    let _ = install();
    true
}

/// Install (or retrieve) the process-local dashboard state.
pub fn install() -> Arc<Dashboard> {
    DASHBOARD
        .get_or_init(|| Arc::new(Dashboard::default()))
        .clone()
}

fn current() -> Option<&'static Arc<Dashboard>> {
    DASHBOARD.get()
}

/// Whether this process has prepared the optional interactive dashboard.
pub fn is_active() -> bool {
    current().is_some()
}

/// Append a formatted runtime event to the bottom log pane. This is deliberately a no-op outside
/// TUI mode, so callers can leave the tracing layer installed without imposing a dashboard cost on
/// ordinary service processes.
pub fn log_line(line: impl Into<String>) {
    if let Some(dashboard) = current() {
        dashboard.log_line(line.into());
    }
}

/// Writer for a `tracing_subscriber::fmt` layer that forwards each formatted event to the dashboard
/// log pane. The caller still controls event filtering and any persistent log sink.
#[derive(Clone, Copy, Default)]
pub struct LogMakeWriter;

impl<'a> MakeWriter<'a> for LogMakeWriter {
    type Writer = LogWriter;

    fn make_writer(&'a self) -> Self::Writer {
        LogWriter::default()
    }
}

/// Buffer one tracing event before atomically adding its non-empty physical lines to the rolling
/// pane. A fmt layer constructs a fresh writer per event, so no mutex is needed here.
#[derive(Default)]
pub struct LogWriter {
    buffer: Vec<u8>,
}

impl Write for LogWriter {
    fn write(&mut self, data: &[u8]) -> io::Result<usize> {
        self.buffer.extend_from_slice(data);
        Ok(data.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

impl Drop for LogWriter {
    fn drop(&mut self) {
        let text = String::from_utf8_lossy(&self.buffer);
        for line in text.lines() {
            let line = line.trim_end();
            if !line.is_empty() {
                log_line(line.to_string());
            }
        }
    }
}

/// Describe a participating component once it has been configured.
pub fn register(component: &'static str, details: impl IntoIterator<Item = String>) {
    let Some(dashboard) = current() else {
        return;
    };
    dashboard.register(component, details);
}

/// Set the most recent unit of work. `expected` is a deadline/ETA, not a promise of completion.
pub fn activity(component: &'static str, what: impl Into<String>, expected: Option<Duration>) {
    if let Some(dashboard) = current() {
        dashboard.activity(component, what.into(), expected);
    }
}

/// Add a monotonically increasing work or transport counter.
pub fn counter(component: &'static str, name: &'static str, amount: u64) {
    if let Some(dashboard) = current() {
        dashboard.counter(component, name, amount);
    }
}

/// Set an instantaneous work or transport gauge.
pub fn gauge(component: &'static str, name: &'static str, value: i64) {
    if let Some(dashboard) = current() {
        dashboard.gauge(component, name, value);
    }
}

/// Adjust an instantaneous gauge while preserving the current value. Suitable for small local
/// concurrency counts such as in-flight effects and HTTP requests.
pub fn gauge_increment(component: &'static str, name: &'static str, amount: i64) {
    if let Some(dashboard) = current() {
        dashboard.gauge_increment(component, name, amount);
    }
}

/// Start the shared dashboard. `q`, `Esc`, or Ctrl-C requests the normal graceful shutdown path.
pub fn spawn(
    dashboard: Arc<Dashboard>,
    is_shutdown: impl Fn() -> bool + Send + Sync + 'static,
    request_shutdown: impl Fn() + Send + Sync + 'static,
) -> thread::JoinHandle<()> {
    thread::spawn(move || {
        let _ = run(dashboard, Arc::new(is_shutdown), Arc::new(request_shutdown));
    })
}

/// Shared mutable dashboard state. This only takes locks while the optional TUI is active; normal
/// production telemetry continues through the binaries' existing OTEL/Prometheus instrumentation.
pub struct Dashboard {
    started: Instant,
    components: RwLock<BTreeMap<&'static str, Component>>,
    logs: RwLock<VecDeque<String>>,
}

impl Default for Dashboard {
    fn default() -> Self {
        Self {
            started: Instant::now(),
            components: RwLock::new(BTreeMap::new()),
            logs: RwLock::new(VecDeque::with_capacity(MAX_LOG_LINES)),
        }
    }
}

#[derive(Clone)]
struct Component {
    details: Vec<String>,
    activity: String,
    activity_started: Instant,
    expected_done: Option<Instant>,
    counters: BTreeMap<&'static str, u64>,
    gauges: BTreeMap<&'static str, i64>,
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

impl Dashboard {
    fn register(&self, name: &'static str, details: impl IntoIterator<Item = String>) {
        let mut components = self
            .components
            .write()
            .unwrap_or_else(|err| err.into_inner());
        let component = components.entry(name).or_default();
        component.details = details.into_iter().collect();
    }

    fn activity(&self, name: &'static str, what: String, expected: Option<Duration>) {
        let mut components = self
            .components
            .write()
            .unwrap_or_else(|err| err.into_inner());
        let component = components.entry(name).or_default();
        component.activity = what;
        component.activity_started = Instant::now();
        component.expected_done = expected.map(|duration| Instant::now() + duration);
    }

    fn counter(&self, name: &'static str, metric: &'static str, amount: u64) {
        let mut components = self
            .components
            .write()
            .unwrap_or_else(|err| err.into_inner());
        let component = components.entry(name).or_default();
        *component.counters.entry(metric).or_default() += amount;
    }

    fn gauge(&self, name: &'static str, metric: &'static str, value: i64) {
        let mut components = self
            .components
            .write()
            .unwrap_or_else(|err| err.into_inner());
        let component = components.entry(name).or_default();
        component.gauges.insert(metric, value);
    }

    fn gauge_increment(&self, name: &'static str, metric: &'static str, amount: i64) {
        let mut components = self
            .components
            .write()
            .unwrap_or_else(|err| err.into_inner());
        let component = components.entry(name).or_default();
        *component.gauges.entry(metric).or_default() += amount;
    }

    fn log_line(&self, line: String) {
        let mut logs = self.logs.write().unwrap_or_else(|err| err.into_inner());
        if logs.len() == MAX_LOG_LINES {
            logs.pop_front();
        }
        logs.push_back(line);
    }

    fn snapshot(&self) -> DashboardSnapshot {
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

struct DashboardSnapshot {
    uptime: Duration,
    components: Vec<ComponentSnapshot>,
    logs: Vec<String>,
}

struct ComponentSnapshot {
    name: &'static str,
    details: Vec<String>,
    activity: String,
    activity_age: Duration,
    expected_remaining: Option<Duration>,
    counters: BTreeMap<&'static str, u64>,
    gauges: BTreeMap<&'static str, i64>,
}

/// The recent host-resource values rendered by the dashboard. The TUI owns this short window so
/// it can graph a local process without depending on a web service or durable store.
#[derive(Default)]
struct ResourceHistory {
    host_cpu: Vec<u64>,
    host_memory: Vec<u64>,
    process_cpu: Vec<u64>,
    network_rx: Vec<u64>,
    network_tx: Vec<u64>,
    disk_io: Vec<u64>,
}

impl ResourceHistory {
    fn push(&mut self, resources: &runinator_models::telemetry::ResourceTelemetry) {
        push_history(&mut self.host_cpu, percent(resources.cpu_percent));
        push_history(&mut self.host_memory, percent(resources.mem_percent));
        push_history(
            &mut self.process_cpu,
            percent(resources.process.cpu_percent),
        );
        push_history(
            &mut self.network_rx,
            rate(resources.network.rx_bytes_per_sec),
        );
        push_history(
            &mut self.network_tx,
            rate(resources.network.tx_bytes_per_sec),
        );
        push_history(
            &mut self.disk_io,
            rate(
                resources
                    .disks
                    .iter()
                    .map(|disk| disk.read_bytes_per_sec + disk.written_bytes_per_sec)
                    .sum(),
            ),
        );
    }
}

fn push_history(history: &mut Vec<u64>, value: u64) {
    if history.len() == RESOURCE_HISTORY_CAPACITY {
        history.remove(0);
    }
    history.push(value);
}

fn percent(value: f32) -> u64 {
    value.clamp(0.0, u64::MAX as f32) as u64
}

fn rate(value: f64) -> u64 {
    if value.is_finite() {
        value.clamp(0.0, u64::MAX as f64) as u64
    } else {
        0
    }
}

fn run(
    dashboard: Arc<Dashboard>,
    is_shutdown: Arc<dyn Fn() -> bool + Send + Sync>,
    request_shutdown: Arc<dyn Fn() + Send + Sync>,
) -> io::Result<()> {
    // `setup_logger` moves tracing into the dashboard while TUI mode is active, but that cannot
    // constrain a dependency that writes directly to stdout or stderr. Take both streams into the
    // rolling log before entering the alternate screen; the returned screen handle stays pointed
    // at the real terminal, so ratatui is the only writer that can paint it.
    let (mut capture, screen) = capture::Capture::install(dashboard.clone())?;
    let backend = CrosstermBackend::new(screen);
    let mut terminal = Terminal::new(backend)?;
    let collector = TelemetryCollector::new();
    let mut resources = collector.sample();
    let mut resource_history = ResourceHistory::default();
    resource_history.push(&resources);
    let mut next_resource_sample = Instant::now() + RESOURCE_SAMPLE_INTERVAL;
    let mut raw = false;
    let mut alternate_screen = false;

    let result = (|| {
        enable_raw_mode()?;
        raw = true;
        execute!(terminal.backend_mut(), EnterAlternateScreen)?;
        alternate_screen = true;

        while !(is_shutdown)() {
            if Instant::now() >= next_resource_sample {
                resources = collector.sample();
                resource_history.push(&resources);
                next_resource_sample = Instant::now() + RESOURCE_SAMPLE_INTERVAL;
            }
            let snapshot = dashboard.snapshot();
            terminal.draw(|frame| render(frame, &snapshot, &resource_history))?;
            if event::poll(Duration::from_millis(250))?
                && let Event::Key(key) = event::read()?
                && key.kind == KeyEventKind::Press
                && (matches!(key.code, KeyCode::Char('q') | KeyCode::Esc)
                    || (key.code == KeyCode::Char('c')
                        && key.modifiers.contains(event::KeyModifiers::CONTROL)))
            {
                (request_shutdown)();
                break;
            }
        }

        Ok(())
    })();

    // Do this on every exit path. In particular, a terminal read/draw error must not strand the
    // user in raw mode, nor may stdout be restored before the alternate screen has gone away.
    let cleanup = leave(&mut terminal, raw, alternate_screen);
    drop(terminal);
    capture.restore();
    result.and(cleanup)
}

fn leave(
    terminal: &mut Terminal<CrosstermBackend<capture::Screen>>,
    raw: bool,
    alternate_screen: bool,
) -> io::Result<()> {
    let mut result = Ok(());
    if alternate_screen {
        result = execute!(terminal.backend_mut(), LeaveAlternateScreen);
    }
    if raw {
        if let Err(error) = disable_raw_mode()
            && result.is_ok()
        {
            result = Err(error);
        }
    }
    if let Err(error) = terminal.show_cursor()
        && result.is_ok()
    {
        result = Err(error);
    }
    result
}

fn render(
    frame: &mut ratatui::Frame,
    snapshot: &DashboardSnapshot,
    resource_history: &ResourceHistory,
) {
    let layout = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3),
            Constraint::Length(4),
            Constraint::Length(4),
            // Let components take whatever remains. The fixed rolling-log pane below stays at the
            // bottom even on a short terminal, which is more useful during a failure than unused
            // whitespace or a truncated footer.
            Constraint::Min(0),
            Constraint::Length(MAX_LOG_LINES as u16 + 2),
            Constraint::Length(1),
        ])
        .split(frame.area());

    let title = Line::from(vec![
        Span::styled(
            " Runinator runtime dashboard ",
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        ),
        Span::raw(format!("uptime {}", human_duration(snapshot.uptime))),
    ]);
    frame.render_widget(
        Paragraph::new(title).block(Block::default().borders(Borders::ALL)),
        layout[0],
    );

    let top_resource_graphs = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Ratio(1, 3); 3])
        .split(layout[1]);
    render_sparkline(
        frame,
        top_resource_graphs[0],
        "Host CPU · 60 sec",
        &resource_history.host_cpu,
        Some(100),
        Color::Cyan,
    );
    render_sparkline(
        frame,
        top_resource_graphs[1],
        "Host RAM · 60 sec",
        &resource_history.host_memory,
        Some(100),
        Color::Magenta,
    );
    render_sparkline(
        frame,
        top_resource_graphs[2],
        "Process CPU · 60 sec",
        &resource_history.process_cpu,
        None,
        Color::Yellow,
    );

    let bottom_resource_graphs = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Ratio(1, 3); 3])
        .split(layout[2]);
    render_sparkline(
        frame,
        bottom_resource_graphs[0],
        "Network in · 60 sec",
        &resource_history.network_rx,
        None,
        Color::Green,
    );
    render_sparkline(
        frame,
        bottom_resource_graphs[1],
        "Network out · 60 sec",
        &resource_history.network_tx,
        None,
        Color::Red,
    );
    render_sparkline(
        frame,
        bottom_resource_graphs[2],
        "Disk I/O · 60 sec",
        &resource_history.disk_io,
        None,
        Color::Blue,
    );

    let component_constraints = snapshot
        .components
        .iter()
        .map(|_| Constraint::Ratio(1, snapshot.components.len() as u32))
        .collect::<Vec<_>>();
    let components = Layout::default()
        .direction(Direction::Vertical)
        .constraints(component_constraints)
        .split(layout[3]);
    for (area, component) in components.iter().zip(&snapshot.components) {
        let mut lines = Vec::new();
        if !component.details.is_empty() {
            lines.push(Line::from(component.details.join("  •  ")));
        }
        let remaining = component
            .expected_remaining
            .map(|duration| format!("  deadline in {}", human_duration(duration)))
            .unwrap_or_default();
        lines.push(Line::from(vec![
            Span::styled("now   ", Style::default().fg(Color::Yellow)),
            Span::raw(&component.activity),
            Span::styled(
                format!(
                    "  for {}{}",
                    human_duration(component.activity_age),
                    remaining
                ),
                Style::default().fg(Color::DarkGray),
            ),
        ]));
        if !component.gauges.is_empty() {
            lines.push(Line::from(format_metrics("live", &component.gauges)));
        }
        if !component.counters.is_empty() {
            lines.push(Line::from(format_metrics("total", &component.counters)));
        }
        frame.render_widget(
            Paragraph::new(lines)
                .wrap(Wrap { trim: true })
                .block(Block::default().borders(Borders::ALL).title(component.name)),
            *area,
        );
    }

    let log_lines = snapshot
        .logs
        .iter()
        .map(|line| Line::from(line.clone()))
        .collect::<Vec<_>>();
    frame.render_widget(
        Paragraph::new(log_lines)
            .block(Block::default().borders(Borders::ALL).title("Recent logs")),
        layout[4],
    );

    frame.render_widget(
        Paragraph::new("q / Esc / Ctrl-C: gracefully stop this runtime")
            .style(Style::default().fg(Color::DarkGray)),
        layout[5],
    );
}

fn render_sparkline(
    frame: &mut ratatui::Frame,
    area: ratatui::layout::Rect,
    title: &'static str,
    values: &[u64],
    max: Option<u64>,
    color: Color,
) {
    let sparkline = Sparkline::default()
        .data(values)
        .block(Block::default().borders(Borders::ALL).title(title))
        .style(Style::default().fg(color));
    let sparkline = match max {
        Some(max) => sparkline.max(max),
        None => sparkline,
    };
    frame.render_widget(sparkline, area);
}

fn format_metrics<T: std::fmt::Display>(
    prefix: &str,
    metrics: &BTreeMap<&'static str, T>,
) -> String {
    let entries = metrics
        .iter()
        .map(|(name, value)| format!("{name}: {value}"))
        .collect::<Vec<_>>()
        .join("   ");
    format!("{prefix:<5} {entries}")
}

fn human_duration(duration: Duration) -> String {
    let seconds = duration.as_secs();
    if seconds >= 3600 {
        format!("{}h {:02}m", seconds / 3600, (seconds % 3600) / 60)
    } else if seconds >= 60 {
        format!("{}m {:02}s", seconds / 60, seconds % 60)
    } else {
        format!("{}s", seconds)
    }
}

#[cfg(test)]
mod tests {
    use super::{Dashboard, RESOURCE_HISTORY_CAPACITY, human_duration, push_history};
    use std::time::Duration;

    #[test]
    fn duration_formatter_keeps_the_dashboard_scannable() {
        assert_eq!(human_duration(Duration::from_secs(125)), "2m 05s");
    }

    #[test]
    fn resource_history_retains_the_latest_minute() {
        let mut history = Vec::new();
        for value in 0..=RESOURCE_HISTORY_CAPACITY as u64 {
            push_history(&mut history, value);
        }

        assert_eq!(history.len(), RESOURCE_HISTORY_CAPACITY);
        assert_eq!(history.first(), Some(&1));
        assert_eq!(history.last(), Some(&(RESOURCE_HISTORY_CAPACITY as u64)));
    }

    #[test]
    fn dashboard_retains_current_work_and_metrics() {
        let dashboard = Dashboard::default();
        dashboard.register("worker", ["relay transport".to_string()]);
        dashboard.activity(
            "worker",
            "executing std.echo".to_string(),
            Some(Duration::from_secs(10)),
        );
        dashboard.counter("worker", "effects received", 1);
        dashboard.gauge_increment("worker", "effects in flight", 1);

        let mut snapshot = dashboard.snapshot();
        let worker = snapshot.components.pop().unwrap();
        assert_eq!(worker.name, "worker");
        assert_eq!(worker.activity, "executing std.echo");
        assert_eq!(worker.counters["effects received"], 1);
        assert_eq!(worker.gauges["effects in flight"], 1);
        assert!(worker.expected_remaining.is_some());
    }

    #[test]
    fn dashboard_retains_only_three_recent_log_lines() {
        let dashboard = Dashboard::default();
        for line in ["one", "two", "three", "four"] {
            dashboard.log_line(line.to_string());
        }

        assert_eq!(dashboard.snapshot().logs, ["two", "three", "four"]);
    }
}
