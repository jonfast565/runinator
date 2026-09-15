//! Shared local and remote operations console.

use std::{
    io,
    path::{Path, PathBuf},
    time::{Duration, Instant},
};

use ratatui::{
    Terminal,
    backend::CrosstermBackend,
    crossterm::{
        event::{self, Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers},
        execute,
        terminal::{EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode},
    },
    layout::{Constraint, Layout},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, List, ListItem, Paragraph},
};
use runinator_api::{AsyncApiClient, StaticLocator};
use runinator_models::{
    diagnostics::{RuntimeLogQuery, RuntimeLogRecord},
    local_runtime::LocalDashboardSnapshot,
    provisioning::{NodeSpec, ProvisionedGroup, ScaleNodesRequest},
    replay::{ReplayOptions, ReplayVerdict},
    replicas::{ReplicaListResponse, ReplicaStatus},
    value::Value,
    workflows::{WorkflowDefinition, WorkflowRun},
};

mod form;
use form::LaunchForm;

pub type Error = Box<dyn std::error::Error + Send + Sync>;
pub type Client = AsyncApiClient<StaticLocator>;

const TABS: [&str; 5] = ["Overview", "Components", "Workflows", "Runs", "Logs"];
const REFRESH: Duration = Duration::from_secs(2);
const LOCAL_REFRESH: Duration = Duration::from_secs(1);

/// Ask for a server only when no existing CLI argument or environment setting selected one.
pub fn select_server(suggestion: Option<&str>) -> Result<String, Error> {
    let mut stdout = io::stdout();
    use std::io::Write;
    match suggestion.filter(|value| !value.trim().is_empty()) {
        Some(value) => write!(stdout, "Runinator server [{value}]: ")?,
        None => write!(stdout, "Runinator server URL: ")?,
    }
    stdout.flush()?;
    let mut value = String::new();
    io::stdin().read_line(&mut value)?;
    let value = value.trim();
    if value.is_empty() {
        if let Some(suggestion) = suggestion.filter(|value| !value.trim().is_empty()) {
            return Ok(suggestion.to_string());
        }
        return Err(io::Error::new(io::ErrorKind::InvalidInput, "a server URL is required").into());
    }
    Ok(value.to_string())
}

/// Run the shared operations console against an already authenticated API client.
pub async fn run(client: Client, server: impl Into<String>) -> Result<(), Error> {
    run_inner(client, server.into(), None).await
}

/// Run the operations console with an atomic local host snapshot augmenting API data.
pub async fn run_with_local_snapshot(
    client: Client,
    server: impl Into<String>,
    snapshot_path: impl Into<PathBuf>,
) -> Result<(), Error> {
    run_inner(client, server.into(), Some(snapshot_path.into())).await
}

async fn run_inner(
    client: Client,
    server: String,
    local_path: Option<PathBuf>,
) -> Result<(), Error> {
    let mut screen = OperationsScreen::enter()?;
    let mut state = State::new(client, server, local_path);
    state.refresh().await;
    let result = state.event_loop(&mut screen.terminal).await;
    screen.leave()?;
    result
}

struct OperationsScreen {
    terminal: Terminal<CrosstermBackend<io::Stdout>>,
    raw: bool,
    alternate: bool,
    _claim: crate::TerminalClaim,
}

impl OperationsScreen {
    fn enter() -> io::Result<Self> {
        let claim = crate::claim_terminal()?;
        let mut terminal = Terminal::new(CrosstermBackend::new(io::stdout()))?;
        enable_raw_mode()?;
        if let Err(error) = execute!(terminal.backend_mut(), EnterAlternateScreen) {
            let _ = disable_raw_mode();
            return Err(error);
        }
        Ok(Self {
            terminal,
            raw: true,
            alternate: true,
            _claim: claim,
        })
    }

    fn leave(&mut self) -> io::Result<()> {
        let mut result = Ok(());
        if self.alternate {
            result = execute!(self.terminal.backend_mut(), LeaveAlternateScreen);
            self.alternate = false;
        }
        if self.raw {
            if let Err(error) = disable_raw_mode()
                && result.is_ok()
            {
                result = Err(error);
            }
            self.raw = false;
        }
        let _ = self.terminal.show_cursor();
        result
    }
}

impl Drop for OperationsScreen {
    fn drop(&mut self) {
        let _ = self.leave();
    }
}

enum InputMode {
    Search(String),
    LaunchJson(String),
    LaunchForm(LaunchForm),
    ReplayStep { run_id: uuid::Uuid, buffer: String },
    Confirm(ConfirmAction),
}

enum ConfirmAction {
    Cancel(uuid::Uuid),
    ScaleDown {
        request: ScaleNodesRequest,
        name: String,
    },
    Replay {
        run_id: uuid::Uuid,
        options: ReplayOptions,
        summary: String,
    },
}

impl ConfirmAction {
    fn prompt(&self) -> String {
        match self {
            Self::Cancel(id) => format!("cancel run {id}? y/N"),
            Self::ScaleDown { request, name } => {
                format!("scale {name} down to {}? y/N", request.desired)
            }
            Self::Replay { summary, .. } => format!("acknowledge replay review: {summary}? y/N"),
        }
    }
}

struct State {
    client: Client,
    server: String,
    tab: usize,
    selected: usize,
    query: String,
    input: Option<InputMode>,
    workflows: Vec<WorkflowDefinition>,
    runs: Vec<WorkflowRun>,
    groups: Vec<ProvisionedGroup>,
    replicas: Option<ReplicaListResponse>,
    logs: Vec<RuntimeLogRecord>,
    logs_paused: bool,
    log_level_filter: Option<String>,
    status: String,
    refreshed_at: Option<Instant>,
    local_path: Option<PathBuf>,
    local_snapshot: Option<LocalDashboardSnapshot>,
    detail: Option<(String, Vec<String>)>,
    detail_scroll: u16,
}

impl State {
    fn new(client: Client, server: String, local_path: Option<PathBuf>) -> Self {
        Self {
            client,
            server,
            tab: 0,
            selected: 0,
            query: String::new(),
            input: None,
            workflows: Vec::new(),
            runs: Vec::new(),
            groups: Vec::new(),
            replicas: None,
            logs: Vec::new(),
            logs_paused: false,
            log_level_filter: None,
            status: "connecting".into(),
            refreshed_at: None,
            local_path,
            local_snapshot: None,
            detail: None,
            detail_scroll: 0,
        }
    }

    async fn event_loop(
        &mut self,
        terminal: &mut Terminal<CrosstermBackend<io::Stdout>>,
    ) -> Result<(), Error> {
        let mut next_refresh = Instant::now() + REFRESH;
        let mut next_local_refresh = Instant::now();
        loop {
            if Instant::now() >= next_local_refresh {
                self.refresh_local();
                next_local_refresh = Instant::now() + LOCAL_REFRESH;
            }
            terminal.draw(|frame| self.draw(frame))?;
            if Instant::now() >= next_refresh {
                self.refresh().await;
                next_refresh = Instant::now() + REFRESH;
            }
            if !event::poll(Duration::from_millis(100))? {
                continue;
            }
            let Event::Key(key) = event::read()? else {
                continue;
            };
            if key.kind != KeyEventKind::Press {
                continue;
            }
            if self.handle_input(key).await? {
                break;
            }
        }
        Ok(())
    }

    async fn handle_input(&mut self, key: KeyEvent) -> Result<bool, Error> {
        if self.detail.is_some() {
            match key.code {
                KeyCode::Esc | KeyCode::Enter | KeyCode::Char('q') => self.detail = None,
                KeyCode::Down | KeyCode::Char('j') => {
                    self.detail_scroll = self.detail_scroll.saturating_add(1)
                }
                KeyCode::Up | KeyCode::Char('k') => {
                    self.detail_scroll = self.detail_scroll.saturating_sub(1)
                }
                _ => {}
            }
            return Ok(false);
        }
        if let Some(InputMode::Confirm(confirm)) = self.input.take() {
            match key.code {
                KeyCode::Char('y') | KeyCode::Char('Y') => self.confirm(confirm).await,
                KeyCode::Esc | KeyCode::Char('n') | KeyCode::Char('N') => {
                    self.status = "action cancelled".into();
                }
                _ => self.input = Some(InputMode::Confirm(confirm)),
            }
            return Ok(false);
        }
        if let Some(mut input) = self.input.take() {
            let buffer = match &mut input {
                InputMode::Search(buffer) | InputMode::LaunchJson(buffer) => buffer,
                InputMode::LaunchForm(form) => &mut form.buffer,
                InputMode::ReplayStep { buffer, .. } => buffer,
                InputMode::Confirm(_) => unreachable!("confirmations are handled above"),
            };
            match key.code {
                KeyCode::Esc => return Ok(false),
                KeyCode::Backspace => {
                    buffer.pop();
                }
                KeyCode::Enter => {
                    match input {
                        InputMode::Search(value) => {
                            self.query = value;
                            self.selected = 0;
                        }
                        InputMode::LaunchJson(value) => self.launch_json(&value).await,
                        InputMode::LaunchForm(mut form) => match form.advance() {
                            Ok(true) => self.launch_form(form).await,
                            Ok(false) => self.input = Some(InputMode::LaunchForm(form)),
                            Err(error) => {
                                self.status = error;
                                self.input = Some(InputMode::LaunchForm(form));
                            }
                        },
                        InputMode::ReplayStep { run_id, buffer } => {
                            let step = buffer.trim();
                            if step.is_empty() {
                                self.status = "a replay step id is required".into();
                            } else {
                                self.replay_from(run_id, Some(step.to_string())).await;
                            }
                        }
                        InputMode::Confirm(_) => unreachable!("confirmations are handled above"),
                    }
                    return Ok(false);
                }
                KeyCode::Char(character) if !key.modifiers.contains(KeyModifiers::CONTROL) => {
                    buffer.push(character);
                }
                _ => {}
            }
            self.input = Some(input);
            return Ok(false);
        }
        match key.code {
            KeyCode::Char('q') | KeyCode::Esc => return Ok(true),
            KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) => return Ok(true),
            KeyCode::Tab | KeyCode::Right | KeyCode::Char('l') => {
                self.tab = (self.tab + 1) % TABS.len();
                self.selected = 0;
            }
            KeyCode::BackTab | KeyCode::Left | KeyCode::Char('h') => {
                self.tab = self.tab.checked_sub(1).unwrap_or(TABS.len() - 1);
                self.selected = 0;
            }
            KeyCode::Down | KeyCode::Char('j') => self.selected = self.selected.saturating_add(1),
            KeyCode::Up | KeyCode::Char('k') => self.selected = self.selected.saturating_sub(1),
            KeyCode::Char('/') => self.input = Some(InputMode::Search(self.query.clone())),
            KeyCode::Char('r') => self.refresh().await,
            KeyCode::Char('n') if self.tab == 2 => {
                let Some(workflow) = self.filtered_workflows().get(self.selected).copied() else {
                    return Ok(false);
                };
                match LaunchForm::new(workflow) {
                    Ok(form) => self.input = Some(InputMode::LaunchForm(form)),
                    Err(error) => self.status = error,
                }
            }
            KeyCode::F(2) if self.tab == 2 => self.input = Some(InputMode::LaunchJson("{}".into())),
            KeyCode::Char('+') if self.tab == 1 => self.scale(1).await,
            KeyCode::Char('-') if self.tab == 1 => self.scale(-1).await,
            KeyCode::Char('p') if self.tab == 3 => self.run_action("pause").await,
            KeyCode::Char('u') if self.tab == 3 => self.run_action("resume").await,
            KeyCode::Char('x') if self.tab == 3 => {
                if let Some(run) = self.filtered_runs().get(self.selected) {
                    self.input = Some(InputMode::Confirm(ConfirmAction::Cancel(run.id)));
                }
            }
            KeyCode::Char('y') if self.tab == 3 => self.replay().await,
            KeyCode::Char('e') if self.tab == 3 => {
                if let Some(run) = self.filtered_runs().get(self.selected) {
                    self.input = Some(InputMode::ReplayStep {
                        run_id: run.id,
                        buffer: String::new(),
                    });
                }
            }
            KeyCode::Enter if self.tab == 3 => self.inspect_run().await,
            KeyCode::Char(' ') if self.tab == 4 => {
                self.logs_paused = !self.logs_paused;
                self.status = if self.logs_paused {
                    "log tail paused".into()
                } else {
                    "log tail following".into()
                };
            }
            KeyCode::Char('f') if self.tab == 4 => {
                self.logs_paused = false;
                self.selected = 0;
                self.status = "log tail following".into();
            }
            KeyCode::Char(level @ ('1' | '2' | '3' | '4' | '5')) if self.tab == 4 => {
                self.log_level_filter = match level {
                    '1' => None,
                    '2' => Some("error".into()),
                    '3' => Some("warn".into()),
                    '4' => Some("info".into()),
                    _ => Some("debug".into()),
                };
                self.selected = 0;
            }
            _ => {}
        }
        self.clamp_selection();
        Ok(false)
    }

    async fn refresh(&mut self) {
        let log_query = RuntimeLogQuery {
            text: (!self.query.is_empty()).then(|| self.query.clone()),
            level: self.log_level_filter.clone(),
            limit: Some(500),
            ..Default::default()
        };
        let refreshed = tokio::time::timeout(Duration::from_secs(4), async {
            tokio::join!(
                self.client.fetch_workflows(),
                self.client.fetch_workflow_runs(None, None),
                self.client.fetch_nodes(),
                self.client.fetch_replicas(None, None),
                self.client.fetch_runtime_logs(&log_query),
            )
        })
        .await;
        let Ok((workflows, runs, groups, replicas, logs)) = refreshed else {
            self.status =
                "connection timed out · showing last successful data · reconnecting".into();
            return;
        };
        let mut failures = Vec::new();
        let mut successes = 0;
        match workflows {
            Ok(value) => {
                self.workflows = value;
                successes += 1;
            }
            Err(error) => failures.push(error.to_string()),
        }
        match runs {
            Ok(value) => {
                self.runs = value;
                successes += 1;
            }
            Err(error) => failures.push(error.to_string()),
        }
        match groups {
            Ok(value) => {
                self.groups = value;
                successes += 1;
            }
            Err(error) => failures.push(error.to_string()),
        }
        match replicas {
            Ok(value) => {
                self.replicas = Some(value);
                successes += 1;
            }
            Err(error) => failures.push(error.to_string()),
        }
        match logs {
            Ok(value) => {
                if !self.logs_paused {
                    self.logs = value.records;
                }
                successes += 1;
            }
            Err(error) => failures.push(error.to_string()),
        }
        if successes > 0 {
            self.refreshed_at = Some(Instant::now());
        }
        self.status = if failures.is_empty() {
            "connected".into()
        } else {
            format!("{} refresh error(s): {}", failures.len(), failures[0])
        };
        self.clamp_selection();
    }

    fn refresh_local(&mut self) {
        let Some(path) = &self.local_path else { return };
        match read_local_snapshot(path) {
            Ok(snapshot) => self.local_snapshot = Some(snapshot),
            Err(error) => {
                if self.local_snapshot.is_none() {
                    self.status = format!("waiting for local dashboard snapshot: {error}");
                }
            }
        }
    }

    async fn launch_json(&mut self, source: &str) {
        let Some(workflow) = self.filtered_workflows().get(self.selected).copied() else {
            return;
        };
        let Some(id) = workflow.id else {
            self.status = "selected workflow has no id".into();
            return;
        };
        let parameters = match serde_json::from_str::<serde_json::Value>(source) {
            Ok(value) => Value::from(value),
            Err(error) => {
                self.status = format!("invalid input JSON: {error}");
                return;
            }
        };
        match self.client.create_workflow_run(id, parameters).await {
            Ok(run) => {
                self.status = format!("started run {}", run.id);
                self.tab = 3;
                self.refresh().await;
            }
            Err(error) => self.status = format!("launch failed: {error}"),
        }
    }

    async fn launch_form(&mut self, form: LaunchForm) {
        let (workflow_id, parameters, run_name) = match form.finish() {
            Ok(value) => value,
            Err(error) => {
                self.status = error;
                return;
            }
        };
        let result = match run_name {
            Some(name) => {
                self.client
                    .create_named_workflow_run(workflow_id, parameters, name)
                    .await
            }
            None => {
                self.client
                    .create_workflow_run(workflow_id, parameters)
                    .await
            }
        };
        match result {
            Ok(run) => {
                self.status = format!("started run {}", run.id);
                self.tab = 3;
                self.refresh().await;
            }
            Err(error) => self.status = format!("launch failed: {error}"),
        }
    }

    async fn run_action(&mut self, action: &str) {
        let Some(run) = self.filtered_runs().get(self.selected).copied() else {
            return;
        };
        let result = match action {
            "pause" => self.client.pause_workflow_run(run.id).await,
            "resume" => self.client.resume_workflow_run(run.id).await,
            _ => self.client.cancel_workflow_run(run.id).await,
        };
        self.status = match result {
            Ok(_) => format!("{action} accepted for {}", run.id),
            Err(error) => format!("{action} failed: {error}"),
        };
        self.refresh().await;
    }

    async fn replay(&mut self) {
        let Some(run) = self.filtered_runs().get(self.selected).copied() else {
            return;
        };
        self.replay_from(run.id, None).await;
    }

    async fn replay_from(&mut self, run_id: uuid::Uuid, from_step_id: Option<String>) {
        let plan = match self
            .client
            .workflow_replay_plan(run_id, from_step_id.as_deref())
            .await
        {
            Ok(plan) => plan,
            Err(error) => {
                self.status = format!("replay review failed: {error}");
                return;
            }
        };
        if plan.verdict == ReplayVerdict::Blocked {
            self.status = format!("replay blocked: {}", plan.reasons.join("; "));
            return;
        }
        let options = ReplayOptions {
            plan_fingerprint: Some(plan.plan_fingerprint),
            acknowledge_review: plan.verdict == ReplayVerdict::Review,
            from_step_id,
        };
        if plan.verdict == ReplayVerdict::Review {
            let summary = plan
                .reasons
                .iter()
                .chain(plan.actions.iter().map(|action| &action.reason))
                .take(3)
                .cloned()
                .collect::<Vec<_>>()
                .join("; ");
            self.input = Some(InputMode::Confirm(ConfirmAction::Replay {
                run_id,
                options,
                summary,
            }));
            return;
        }
        self.status = match self
            .client
            .replay_workflow_run_reviewed(run_id, &options)
            .await
        {
            Ok(replayed) => format!("replay started as {}", replayed.id),
            Err(error) => format!("replay failed: {error}"),
        };
        self.refresh().await;
    }

    async fn scale(&mut self, direction: i32) {
        let local_count = self
            .local_snapshot
            .as_ref()
            .map_or(0, |value| value.components.len());
        let Some(group_index) = self.selected.checked_sub(local_count) else {
            self.status = "local embedded components are observed through their host".into();
            return;
        };
        let Some(group) = self.filtered_groups().get(group_index).copied() else {
            return;
        };
        if !group.manageable {
            self.status = format!("{} is read-only", group.name);
            return;
        }
        let desired = if direction > 0 {
            group.desired.saturating_add(1)
        } else {
            group.desired.saturating_sub(1).max(group.min_desired)
        };
        let request = ScaleNodesRequest {
            backend: group.backend,
            kind: group.kind,
            desired,
            spec: NodeSpec {
                group: Some(group.name.clone()),
                ..Default::default()
            },
        };
        if direction < 0 {
            self.input = Some(InputMode::Confirm(ConfirmAction::ScaleDown {
                request,
                name: group.name.clone(),
            }));
            return;
        }
        self.status = match self.client.scale_nodes(&request).await {
            Ok(value) => format!("{} desired count is {}", value.name, value.desired),
            Err(error) => format!("scale failed: {error}"),
        };
        self.refresh().await;
    }

    async fn confirm(&mut self, action: ConfirmAction) {
        match action {
            ConfirmAction::Cancel(run_id) => {
                self.status = match self.client.cancel_workflow_run(run_id).await {
                    Ok(_) => format!("cancel accepted for {run_id}"),
                    Err(error) => format!("cancel failed: {error}"),
                };
            }
            ConfirmAction::ScaleDown { request, .. } => {
                self.status = match self.client.scale_nodes(&request).await {
                    Ok(value) => format!("{} desired count is {}", value.name, value.desired),
                    Err(error) => format!("scale failed: {error}"),
                };
            }
            ConfirmAction::Replay {
                run_id, options, ..
            } => {
                // The options contain the reviewed fingerprint. A changed plan is rejected by the
                // server, forcing the operator to refresh and review the new plan.
                self.status = match self
                    .client
                    .replay_workflow_run_reviewed(run_id, &options)
                    .await
                {
                    Ok(replayed) => format!("replay started as {}", replayed.id),
                    Err(error) => format!("replay failed: {error}"),
                };
            }
        }
        self.refresh().await;
    }

    async fn inspect_run(&mut self) {
        let Some(run_id) = self.filtered_runs().get(self.selected).map(|run| run.id) else {
            return;
        };
        self.status = format!("loading run {run_id}");
        let log_query = RuntimeLogQuery {
            workflow_run_id: Some(run_id),
            limit: Some(500),
            ..Default::default()
        };
        let (run, continuations, effects, journal, transitions, logs) = tokio::join!(
            self.client.fetch_workflow_run(run_id),
            self.client.fetch_workflow_continuations(run_id),
            self.client.fetch_workflow_effects(run_id),
            self.client.fetch_workflow_journal(run_id),
            self.client.fetch_workflow_run_transitions(run_id),
            self.client.fetch_runtime_logs(&log_query),
        );
        let mut lines = Vec::new();
        append_json(&mut lines, "run", run);
        append_json(&mut lines, "continuations", continuations);
        append_json(&mut lines, "effects", effects);
        append_json(&mut lines, "journal", journal);
        append_json(&mut lines, "transitions", transitions);
        append_json(&mut lines, "correlated logs", logs.map(|page| page.records));
        self.detail = Some((format!(" Run {run_id} · Esc closes "), lines));
        self.detail_scroll = 0;
        self.status = "run detail loaded".into();
    }

    fn filtered_workflows(&self) -> Vec<&WorkflowDefinition> {
        self.workflows
            .iter()
            .filter(|item| matches_query(&item.name, &self.query))
            .collect()
    }

    fn filtered_runs(&self) -> Vec<&WorkflowRun> {
        self.runs
            .iter()
            .filter(|item| {
                matches_query(
                    &format!(
                        "{} {} {}",
                        item.id,
                        item.status.as_str(),
                        item.name.as_deref().unwrap_or_default()
                    ),
                    &self.query,
                )
            })
            .collect()
    }

    fn filtered_groups(&self) -> Vec<&ProvisionedGroup> {
        self.groups
            .iter()
            .filter(|item| {
                matches_query(
                    &format!("{} {}", item.name, item.kind.as_str()),
                    &self.query,
                )
            })
            .collect()
    }

    fn item_count(&self) -> usize {
        match self.tab {
            1 => {
                self.local_snapshot
                    .as_ref()
                    .map_or(0, |value| value.components.len())
                    + self.filtered_groups().len()
                    + self
                        .replicas
                        .as_ref()
                        .map_or(0, |value| value.replicas.len())
            }
            2 => self.filtered_workflows().len(),
            3 => self.filtered_runs().len(),
            4 => self.logs.len().max(1),
            _ => 1,
        }
    }

    fn clamp_selection(&mut self) {
        self.selected = self.selected.min(self.item_count().saturating_sub(1));
    }

    fn draw(&self, frame: &mut ratatui::Frame) {
        let [header, body, status, footer] = Layout::vertical([
            Constraint::Length(3),
            Constraint::Min(4),
            Constraint::Length(3),
            Constraint::Length(1),
        ])
        .areas(frame.area());
        let tabs = TABS
            .iter()
            .enumerate()
            .flat_map(|(index, tab)| {
                let style = if index == self.tab {
                    Style::default()
                        .fg(Color::Cyan)
                        .add_modifier(Modifier::BOLD)
                } else {
                    Style::default().fg(Color::DarkGray)
                };
                [Span::styled(format!(" {tab} "), style), Span::raw(" ")]
            })
            .collect::<Vec<_>>();
        let freshness = self.refreshed_at.map_or("connecting", |at| {
            if at.elapsed() > Duration::from_secs(6) {
                "stale"
            } else {
                "connected"
            }
        });
        frame.render_widget(
            Paragraph::new(Line::from(tabs)).block(Block::default().borders(Borders::ALL).title(
                format!(
                    " Runinator · {} · {} · {} ",
                    self.server,
                    self.organization_label(),
                    freshness
                ),
            )),
            header,
        );
        self.draw_body(frame, body);
        frame.render_widget(
            Paragraph::new(self.status.clone())
                .block(Block::default().borders(Borders::ALL).title(" Status ")),
            status,
        );
        let prompt = match &self.input {
            Some(InputMode::Search(value)) => format!("search: {value}_  · Enter apply · Esc cancel"),
            Some(InputMode::LaunchJson(value)) => format!("advanced workflow input JSON: {value}_  · Enter launch · Esc cancel"),
            Some(InputMode::LaunchForm(form)) => format!("{}  · Enter next · Esc cancel", form.prompt()),
            Some(InputMode::ReplayStep { buffer, .. }) => format!("replay from step id: {buffer}_ · Enter review · Esc cancel"),
            Some(InputMode::Confirm(confirm)) => confirm.prompt(),
            None if self.tab == 4 => "j/k scroll · / text search · 1 all · 2 error · 3 warn · 4 info · 5 debug · Space pause · f follow · q quit".into(),
            None => "Tab/←/→ views · j/k select · / search · r refresh · q quit · n guided launch · F2 JSON · +/- scale · p/u/x run · y replay · e replay step".into(),
        };
        frame.render_widget(
            Paragraph::new(prompt).style(Style::default().fg(Color::DarkGray)),
            footer,
        );
    }

    fn draw_body(&self, frame: &mut ratatui::Frame, area: ratatui::layout::Rect) {
        if let Some((title, lines)) = &self.detail {
            frame.render_widget(
                Paragraph::new(lines.join("\n"))
                    .scroll((self.detail_scroll, 0))
                    .block(Block::default().borders(Borders::ALL).title(title.as_str())),
                area,
            );
            return;
        }
        let lines = match self.tab {
            0 => self.overview_lines(),
            1 => self.component_lines(),
            2 => self.workflow_lines(),
            3 => self.run_lines(),
            _ => self.log_lines(),
        };
        let visible = usize::from(area.height.saturating_sub(2)).max(1);
        let start = self.selected.saturating_sub(visible.saturating_sub(1));
        let items = lines
            .into_iter()
            .enumerate()
            .skip(start)
            .take(visible)
            .map(|(index, line)| {
                let style = if index == self.selected && self.tab != 0 {
                    Style::default().bg(Color::DarkGray)
                } else {
                    Style::default()
                };
                ListItem::new(line).style(style)
            })
            .collect::<Vec<_>>();
        frame.render_widget(
            List::new(items).block(
                Block::default()
                    .borders(Borders::ALL)
                    .title(format!(" {} ", TABS[self.tab])),
            ),
            area,
        );
    }

    fn overview_lines(&self) -> Vec<Line<'static>> {
        let active = self
            .runs
            .iter()
            .filter(|run| run.status.is_active())
            .count();
        let failed = self
            .runs
            .iter()
            .filter(|run| run.status.as_str() == "failed")
            .count();
        let replicas = self
            .replicas
            .as_ref()
            .map_or(0, |value| value.replicas.len());
        let stale = self.replicas.as_ref().map_or(0, |value| {
            value
                .replicas
                .iter()
                .filter(|replica| replica.status != ReplicaStatus::Live)
                .count()
        });
        vec![
            format!("workflows: {}", self.workflows.len()),
            format!(
                "runs: {} active · {} failed · {} total",
                active,
                failed,
                self.runs.len()
            ),
            format!(
                "components: {} groups · {} replicas · {} stale/offline",
                self.groups.len(),
                replicas,
                stale
            ),
            format!(
                "local host: {}",
                self.local_snapshot
                    .as_ref()
                    .map(|snapshot| format!(
                        "{} · {} components · {}",
                        snapshot.host_id,
                        snapshot.components.len(),
                        snapshot.updated_at
                    ))
                    .unwrap_or_else(|| "not attached".into())
            ),
            format!(
                "filter: {}",
                if self.query.is_empty() {
                    "none"
                } else {
                    &self.query
                }
            ),
        ]
        .into_iter()
        .map(Line::raw)
        .collect()
    }

    fn organization_label(&self) -> String {
        let mut orgs = self.workflows.iter().filter_map(|workflow| workflow.org_id);
        let Some(first) = orgs.next() else {
            return "platform scope".into();
        };
        if orgs.all(|id| id == first) {
            format!("org {first}")
        } else {
            "multiple organizations".into()
        }
    }

    fn component_lines(&self) -> Vec<Line<'static>> {
        let mut lines = self
            .local_snapshot
            .as_ref()
            .into_iter()
            .flat_map(|snapshot| {
                snapshot.components.iter().map(|component| {
                    format!(
                        "local {:<22} {:<12} {:<10} up={}s restarts={}{}",
                        component.id,
                        component.kind,
                        component.status,
                        component.uptime_seconds.unwrap_or_default(),
                        component.restarts,
                        component
                            .last_error
                            .as_ref()
                            .map(|error| format!(" error={error}"))
                            .unwrap_or_default(),
                    )
                })
            })
            .collect::<Vec<_>>();
        lines.extend(self.filtered_groups().into_iter().map(|group| {
            format!(
                "group {:<22} {:<12} {}/{}{}",
                group.name,
                group.kind.as_str(),
                group.available,
                group.desired,
                if group.manageable {
                    "  +/-"
                } else {
                    "  read-only"
                }
            )
        }));
        if let Some(replicas) = &self.replicas {
            lines.extend(replicas.replicas.iter().map(|replica| {
                format!(
                    "replica {:<20} {:<12} {:?}  {}",
                    replica
                        .display_name
                        .as_deref()
                        .unwrap_or(&replica.instance_id),
                    replica.replica_type.as_str(),
                    replica.status,
                    replica.last_heartbeat_at
                )
            }));
        }
        lines.into_iter().map(Line::raw).collect()
    }

    fn workflow_lines(&self) -> Vec<Line<'static>> {
        self.filtered_workflows()
            .into_iter()
            .map(|workflow| {
                format!(
                    "{:<42} v{}  {}",
                    workflow.name,
                    workflow.version,
                    if workflow.enabled {
                        "enabled"
                    } else {
                        "disabled"
                    }
                )
            })
            .map(Line::raw)
            .collect()
    }

    fn run_lines(&self) -> Vec<Line<'static>> {
        self.filtered_runs()
            .into_iter()
            .map(|run| {
                format!(
                    "{}  {:<18} {:<28} {}",
                    run.id,
                    run.status.as_str(),
                    run.name.as_deref().unwrap_or(""),
                    run.message.as_deref().unwrap_or("")
                )
            })
            .map(Line::raw)
            .collect()
    }

    fn log_lines(&self) -> Vec<Line<'static>> {
        if self.logs.is_empty() {
            return vec![Line::raw(
                "No matching centralized runtime logs are available.",
            )];
        }
        self.logs
            .iter()
            .map(|record| {
                let mut parser = crate::ansi::AnsiParser::default();
                let mut message = parser.parse_line(&record.message).to_ratatui_line();
                let color = match record.level.to_ascii_lowercase().as_str() {
                    "error" => Color::LightRed,
                    "warn" | "warning" => Color::LightYellow,
                    "debug" | "trace" => Color::DarkGray,
                    _ => Color::Gray,
                };
                let mut spans = vec![
                    Span::styled(
                        record.occurred_at.format("%H:%M:%S").to_string(),
                        Style::default().fg(Color::DarkGray),
                    ),
                    Span::raw(" "),
                    Span::styled(
                        format!("{:<5}", record.level),
                        Style::default().fg(color).add_modifier(Modifier::BOLD),
                    ),
                    Span::raw(format!(" {:<18} ", record.source)),
                ];
                spans.append(&mut message.spans);
                Line::from(spans)
            })
            .collect()
    }
}

fn matches_query(value: &str, query: &str) -> bool {
    query.is_empty() || value.to_lowercase().contains(&query.to_lowercase())
}

fn append_json<T: serde::Serialize>(
    lines: &mut Vec<String>,
    label: &str,
    result: std::result::Result<T, runinator_api::ApiError>,
) {
    lines.push(format!("{label}:"));
    match result {
        Ok(value) => match serde_json::to_string_pretty(&value) {
            Ok(value) => lines.extend(value.lines().map(|line| format!("  {line}"))),
            Err(error) => lines.push(format!("  unable to render: {error}")),
        },
        Err(error) => lines.push(format!("  unavailable: {error}")),
    }
    lines.push(String::new());
}

fn read_local_snapshot(path: &Path) -> io::Result<LocalDashboardSnapshot> {
    let bytes = std::fs::read(path)?;
    serde_json::from_slice(&bytes)
        .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))
}
