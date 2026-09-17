//! The supervisor's local terminal dashboard.
//!
//! This is deliberately a view over `state.json`, rather than a second supervision loop. The
//! daemon remains the sole writer of process state, which makes the same dashboard usable for an
//! attached `status --watch` client and for a foreground supervisor.

use std::{
    collections::{BTreeMap, VecDeque},
    fs,
    io::{self, IsTerminal},
    path::Path,
    time::{Duration, Instant},
};

use ratatui::{
    Terminal,
    backend::CrosstermBackend,
    crossterm::{
        event::{self, Event, KeyCode, KeyEventKind, KeyModifiers},
        execute,
        terminal::{EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode},
    },
    layout::{Constraint, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Cell, Paragraph, Row, Sparkline, Table, TableState, Wrap},
};

use serde::{Deserialize, Serialize};

pub type DynError = Box<dyn std::error::Error + Send + Sync>;

fn read_snapshot(path: &Path) -> Result<StateSnapshot, DynError> {
    Ok(serde_json::from_str(&fs::read_to_string(path)?)?)
}

/// State is sampled twice per second, so 120 samples makes the graph a one-minute window.
const HISTORY_CAPACITY: usize = 120;
const REFRESH_INTERVAL: Duration = Duration::from_millis(500);
const FIXED_DASHBOARD_ROWS: u16 = 3 + 5 + 5 + 1;
const PROCESS_TABLE_CHROME_ROWS: u16 = 4;

/// What leaving the dashboard means depends on who owns the supervision loop.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DashboardMode {
    /// `status --watch`: closing only detaches this reader.
    Monitor,
    /// `start --foreground`: closing asks the foreground supervisor to stop gracefully.
    ForegroundSupervisor,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DashboardAction {
    Continue,
    CloseMonitor,
    StopSupervisor,
}

/// An alternate-screen process monitor. It owns terminal restoration so a failed draw or read
/// cannot strand a shell in raw mode.

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum StatusTone {
    Good,
    Attention,
    Bad,
    Inactive,
}

impl StatusTone {
    fn color(self) -> Color {
        match self {
            StatusTone::Good => Color::Green,
            StatusTone::Attention => Color::Yellow,
            StatusTone::Bad => Color::Red,
            StatusTone::Inactive => Color::DarkGray,
        }
    }

    fn marker(self) -> &'static str {
        match self {
            StatusTone::Good => "●",
            StatusTone::Attention => "●",
            StatusTone::Bad => "●",
            StatusTone::Inactive => "○",
        }
    }
}

fn status_tone(status: &str) -> StatusTone {
    match status {
        "running" => StatusTone::Good,
        "starting" | "backoff" | "stopping" => StatusTone::Attention,
        "stopped" | "exited" => StatusTone::Inactive,
        "failed" => StatusTone::Bad,
        _ => StatusTone::Bad,
    }
}

fn push_sample(samples: &mut VecDeque<u64>, sample: u64) {
    if samples.len() == HISTORY_CAPACITY {
        samples.pop_front();
    }
    samples.push_back(sample);
}

fn summarize(processes: &[ProcessSnapshot]) -> Summary {
    let mut summary = Summary::default();
    for process in processes {
        match status_tone(&process.status) {
            StatusTone::Good => summary.healthy += 1,
            StatusTone::Attention => summary.attention += 1,
            StatusTone::Bad => summary.bad += 1,
            StatusTone::Inactive => summary.inactive += 1,
        }
    }
    summary
}

fn render(
    frame: &mut ratatui::Frame,
    snapshot: Option<&StateSnapshot>,
    warning: Option<&str>,
    selected: usize,
    history: &MetricHistory,
    mode: DashboardMode,
    process_page_rows: usize,
) {
    let [header, metrics, processes, detail, footer] = Layout::vertical([
        Constraint::Length(3),
        Constraint::Length(5),
        Constraint::Min(8),
        Constraint::Length(5),
        Constraint::Length(1),
    ])
    .areas(frame.area());

    render_header(frame, header, snapshot, warning);
    render_metrics(frame, metrics, snapshot, history);
    render_processes(frame, processes, snapshot, selected, process_page_rows);
    render_detail(frame, detail, snapshot, selected);
    render_footer(
        frame,
        footer,
        mode,
        selected,
        snapshot.map_or(0, |state| state.processes.len()),
        process_page_rows,
    );
}

fn render_header(
    frame: &mut ratatui::Frame,
    area: Rect,
    snapshot: Option<&StateSnapshot>,
    warning: Option<&str>,
) {
    let content = match snapshot {
        Some(snapshot) => {
            let summary = summarize(&snapshot.processes);
            let overall = if summary.bad > 0 {
                Color::Red
            } else if summary.attention > 0 {
                Color::Yellow
            } else if summary.healthy > 0 {
                Color::Green
            } else {
                Color::DarkGray
            };
            Line::from(vec![
                Span::styled(
                    format!(" {} healthy", summary.healthy),
                    Style::new().fg(Color::Green).bold(),
                ),
                Span::styled(
                    format!("  {} attention", summary.attention),
                    Style::new().fg(Color::Yellow),
                ),
                Span::styled(
                    format!("  {} failed", summary.bad),
                    Style::new().fg(Color::Red).bold(),
                ),
                Span::styled(
                    format!("  {} inactive", summary.inactive),
                    Style::new().fg(Color::DarkGray),
                ),
                Span::styled(
                    format!("   · supervisor PID {}", snapshot.supervisor_pid),
                    Style::new().fg(overall),
                ),
                Span::styled(
                    format!("   · updated {}", snapshot.updated_at),
                    Style::new().fg(Color::DarkGray),
                ),
            ])
        }
        None => Line::styled(
            " Waiting for a supervisor state snapshot…",
            Style::new().fg(Color::Yellow),
        ),
    };

    let mut lines = vec![content];
    if let Some(warning) = warning {
        lines.push(Line::styled(
            format!(" {warning}"),
            Style::new().fg(Color::Yellow),
        ));
    }
    frame.render_widget(
        Paragraph::new(lines).block(Block::new().borders(Borders::ALL).title(Span::styled(
            " Runinator Supervisor ",
            Style::new().fg(Color::Cyan).bold(),
        ))),
        area,
    );
}

fn render_metrics(
    frame: &mut ratatui::Frame,
    area: Rect,
    snapshot: Option<&StateSnapshot>,
    history: &MetricHistory,
) {
    let [health_area, restart_area] =
        Layout::horizontal([Constraint::Ratio(1, 2), Constraint::Ratio(1, 2)]).areas(area);
    let total = snapshot.map_or(0, |state| state.processes.len());
    let healthy = snapshot.map_or(0, |state| summarize(&state.processes).healthy);
    let health_color = if total > 0 && healthy == total {
        Color::Green
    } else if healthy > 0 {
        Color::Yellow
    } else {
        Color::Red
    };
    let health_values = history.healthy_percent.iter().copied().collect::<Vec<_>>();
    frame.render_widget(
        Sparkline::default()
            .data(&health_values)
            .max(100)
            .style(Style::new().fg(health_color))
            .block(Block::new().borders(Borders::ALL).title(format!(
                " Health · {healthy}/{total} running · last minute "
            ))),
        health_area,
    );

    let restart_count = history.restart_count();
    let restart_color = if restart_count > 0 {
        Color::Red
    } else {
        Color::Green
    };
    let restart_values = history.restart_events.iter().copied().collect::<Vec<_>>();
    frame.render_widget(
        Sparkline::default()
            .data(&restart_values)
            .style(Style::new().fg(restart_color))
            .block(
                Block::new()
                    .borders(Borders::ALL)
                    .title(format!(" Restart events · {restart_count} in this minute ")),
            ),
        restart_area,
    );
}

fn render_processes(
    frame: &mut ratatui::Frame,
    area: Rect,
    snapshot: Option<&StateSnapshot>,
    selected: usize,
    process_page_rows: usize,
) {
    let Some(snapshot) = snapshot else {
        frame.render_widget(
            Paragraph::new("No process data yet.")
                .block(Block::new().borders(Borders::ALL).title(" Processes ")),
            area,
        );
        return;
    };

    let header = Row::new([
        "Process", "State", "PID", "Uptime", "Restarts", "Exit", "Command",
    ])
    .style(Style::new().fg(Color::Cyan).bold())
    .bottom_margin(1);
    let rows = snapshot.processes.iter().map(process_row);
    let (page, page_count) =
        process_page_position(selected, snapshot.processes.len(), process_page_rows);
    let table = Table::new(
        rows,
        [
            Constraint::Length(20),
            Constraint::Length(13),
            Constraint::Length(8),
            Constraint::Length(10),
            Constraint::Length(10),
            Constraint::Length(7),
            Constraint::Min(16),
        ],
    )
    .header(header)
    .block(Block::new().borders(Borders::ALL).title(format!(
        " Processes · {} managed · page {page}/{page_count} ",
        snapshot.processes.len(),
    )))
    .row_highlight_style(
        Style::new()
            .bg(Color::DarkGray)
            .add_modifier(Modifier::BOLD),
    )
    .highlight_symbol("› ");

    let mut state = TableState::default();
    if !snapshot.processes.is_empty() {
        state.select(Some(selected.min(snapshot.processes.len() - 1)));
    }
    frame.render_stateful_widget(table, area, &mut state);
}

fn process_row(process: &ProcessSnapshot) -> Row<'_> {
    let tone = status_tone(&process.status);
    let state = Line::from(vec![
        Span::styled(
            format!("{} ", tone.marker()),
            Style::new().fg(tone.color()).bold(),
        ),
        Span::styled(
            process.status.to_ascii_uppercase(),
            Style::new().fg(tone.color()).bold(),
        ),
    ]);
    Row::new(vec![
        Cell::from(process.name.as_str()),
        Cell::from(state),
        Cell::from(
            process
                .pid
                .map(|pid| pid.to_string())
                .unwrap_or_else(|| "-".to_string()),
        ),
        Cell::from(
            process
                .uptime_seconds
                .map(format_uptime)
                .unwrap_or_else(|| "-".to_string()),
        ),
        Cell::from(process.restarts.to_string()),
        Cell::from(
            process
                .last_exit_code
                .map(|code| code.to_string())
                .unwrap_or_else(|| "-".to_string()),
        ),
        Cell::from(process.command.as_str()),
    ])
}

fn render_detail(
    frame: &mut ratatui::Frame,
    area: Rect,
    snapshot: Option<&StateSnapshot>,
    selected: usize,
) {
    let process = snapshot.and_then(|state| state.processes.get(selected));
    let (title, lines) = match process {
        Some(process) => {
            let error = process
                .last_error
                .as_deref()
                .map(|message| {
                    Line::from(vec![
                        Span::styled("Error: ", Style::new().fg(Color::Red).bold()),
                        Span::styled(message, Style::new().fg(Color::Red)),
                    ])
                })
                .unwrap_or_else(|| {
                    Line::styled(
                        format!(
                            "Log: {}{}",
                            process.log_file,
                            process
                                .last_exit_code
                                .map(|code| format!(" · last exit {code}"))
                                .unwrap_or_default()
                        ),
                        Style::new().fg(Color::DarkGray),
                    )
                });
            (
                format!(" {} ", process.name),
                vec![
                    Line::from(vec![
                        Span::styled("Command: ", Style::new().fg(Color::Cyan)),
                        Span::raw(&process.command),
                    ]),
                    Line::from(vec![
                        Span::styled("Working directory: ", Style::new().fg(Color::Cyan)),
                        Span::raw(&process.cwd),
                    ]),
                    error,
                ],
            )
        }
        None => (
            " Process details ".to_string(),
            vec![Line::styled(
                "Use ↑/↓ to inspect a managed process once one is available.",
                Style::new().fg(Color::DarkGray),
            )],
        ),
    };
    frame.render_widget(
        Paragraph::new(lines)
            .wrap(Wrap { trim: true })
            .block(Block::new().borders(Borders::ALL).title(title)),
        area,
    );
}

fn render_footer(
    frame: &mut ratatui::Frame,
    area: Rect,
    mode: DashboardMode,
    selected: usize,
    process_count: usize,
    process_page_rows: usize,
) {
    let close = match mode {
        DashboardMode::Monitor => "q / Esc close monitor",
        DashboardMode::ForegroundSupervisor => "q / Esc stop supervisor",
    };
    let (page, page_count) = process_page_position(selected, process_count, process_page_rows);
    frame.render_widget(
        Paragraph::new(format!(
            " ↑/↓ or j/k select · ←/→ or PgUp/PgDn page {page}/{page_count} · {close} "
        ))
        .style(Style::new().fg(Color::DarkGray)),
        area,
    );
}

fn visible_process_rows(frame_height: u16) -> usize {
    usize::from(
        frame_height
            .saturating_sub(FIXED_DASHBOARD_ROWS + PROCESS_TABLE_CHROME_ROWS)
            .max(1),
    )
}

fn previous_process_page(selected: usize, process_page_rows: usize) -> usize {
    selected.saturating_sub(process_page_rows.max(1))
}

fn next_process_page(selected: usize, process_count: usize, process_page_rows: usize) -> usize {
    selected
        .saturating_add(process_page_rows.max(1))
        .min(process_count.saturating_sub(1))
}

fn process_page_position(
    selected: usize,
    process_count: usize,
    process_page_rows: usize,
) -> (usize, usize) {
    let process_page_rows = process_page_rows.max(1);
    let page_count = (process_count / process_page_rows
        + usize::from(!process_count.is_multiple_of(process_page_rows)))
    .max(1);
    let page =
        (selected.min(process_count.saturating_sub(1)) / process_page_rows + 1).min(page_count);
    (page, page_count)
}

fn format_uptime(seconds: u64) -> String {
    let hours = seconds / 3600;
    let minutes = (seconds % 3600) / 60;
    let secs = seconds % 60;
    format!("{hours:02}:{minutes:02}:{secs:02}")
}

#[cfg(test)]
mod tests {
    use super::{
        DashboardMode, HISTORY_CAPACITY, MetricHistory, StatusTone, next_process_page,
        previous_process_page, process_page_position, render, status_tone,
    };
    use super::{ProcessSnapshot, StateSnapshot};
    use ratatui::{Terminal, backend::TestBackend, style::Color};

    fn process(name: &str, status: &str, restarts: u32) -> ProcessSnapshot {
        ProcessSnapshot {
            name: name.to_string(),
            status: status.to_string(),
            pid: Some(42),
            restarts,
            uptime_seconds: Some(75),
            last_exit_code: None,
            last_error: None,
            started_at: None,
            command: "runinator-worker".to_string(),
            cwd: "/tmp".to_string(),
            log_file: "/tmp/worker.log".to_string(),
        }
    }

    fn snapshot(processes: Vec<ProcessSnapshot>) -> StateSnapshot {
        StateSnapshot {
            supervisor_pid: 42,
            config_path: "runinator-supervisor.json".to_string(),
            started_at: "2026-08-25T12:00:00Z".to_string(),
            updated_at: "2026-08-25T12:00:01Z".to_string(),
            processes,
        }
    }

    #[test]
    fn status_colours_treat_running_as_good_and_failed_as_bad() {
        assert_eq!(status_tone("running"), StatusTone::Good);
        assert_eq!(status_tone("backoff"), StatusTone::Attention);
        assert_eq!(status_tone("failed"), StatusTone::Bad);
        assert_eq!(status_tone("stopped"), StatusTone::Inactive);
        assert_eq!(status_tone("running").color(), Color::Green);
        assert_eq!(status_tone("failed").color(), Color::Red);
    }

    #[test]
    fn metric_history_tracks_health_and_new_restart_events() {
        let mut history = MetricHistory::default();
        history.observe(&snapshot(vec![
            process("worker", "running", 2),
            process("waker", "failed", 1),
        ]));
        history.observe(&snapshot(vec![
            process("worker", "running", 3),
            process("waker", "running", 1),
        ]));

        assert_eq!(history.healthy_percent.back(), Some(&100));
        assert_eq!(history.restart_count(), 1);
    }

    #[test]
    fn metric_history_keeps_only_the_latest_minute() {
        let mut history = MetricHistory::default();
        for restarts in 0..=HISTORY_CAPACITY as u32 {
            history.observe(&snapshot(vec![process("worker", "running", restarts)]));
        }

        assert_eq!(history.healthy_percent.len(), HISTORY_CAPACITY);
        assert_eq!(history.restart_events.len(), HISTORY_CAPACITY);
    }

    #[test]
    fn process_navigation_moves_by_the_visible_page() {
        assert_eq!(previous_process_page(7, 5), 2);
        assert_eq!(previous_process_page(2, 5), 0);
        assert_eq!(next_process_page(2, 13, 5), 7);
        assert_eq!(next_process_page(11, 13, 5), 12);
        assert_eq!(process_page_position(7, 13, 5), (2, 3));
        assert_eq!(process_page_position(0, 0, 5), (1, 1));
    }

    #[test]
    fn dashboard_renders_process_states_and_live_graphs() {
        let state = snapshot(vec![
            process("worker", "running", 0),
            process("waker", "failed", 1),
        ]);
        let mut history = MetricHistory::default();
        history.observe(&state);
        let mut terminal = Terminal::new(TestBackend::new(120, 28)).expect("test terminal builds");
        terminal
            .draw(|frame| {
                render(
                    frame,
                    Some(&state),
                    None,
                    0,
                    &history,
                    DashboardMode::Monitor,
                    10,
                );
            })
            .expect("dashboard renders");
        let buffer = terminal.backend().buffer().clone();
        let lines = (0..buffer.area.height)
            .map(|row| {
                (0..buffer.area.width)
                    .map(|column| buffer[(column, row)].symbol())
                    .collect::<String>()
            })
            .collect::<Vec<_>>();

        assert!(lines.iter().any(|line| line.contains("Health")));
        assert!(lines.iter().any(|line| line.contains("Restart events")));
        assert!(lines.iter().any(|line| line.contains("RUNNING")));
        assert!(lines.iter().any(|line| line.contains("FAILED")));
    }
}

mod state_snapshot;
pub use state_snapshot::StateSnapshot;

mod process_snapshot;
pub use process_snapshot::ProcessSnapshot;

mod supervisor_tui;
pub use supervisor_tui::SupervisorTui;

mod metric_history;
use metric_history::MetricHistory;

mod summary;
use summary::Summary;
