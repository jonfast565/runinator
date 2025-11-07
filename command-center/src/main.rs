use anyhow::Result;
use chrono::{DateTime, Utc};
use crossterm::event::{self, Event as CEvent, KeyCode, KeyEvent, KeyEventKind, KeyModifiers};
use crossterm::terminal::{
    EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode,
};
use crossterm::{ExecutableCommand, execute};
use ratatui::Terminal;
use ratatui::backend::CrosstermBackend;
use ratatui::layout::{Alignment, Constraint, Direction, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span, Text};
use ratatui::widgets::{
    Block, Borders, Cell, Clear, List, ListItem, ListState, Paragraph, Row, Table, TableState, Wrap,
};
use runinator_api::BlockingApiClient;
use runinator_models::core::ScheduledTask as ApiScheduledTask;
use serde::{Deserialize, Serialize};
use std::cmp::min;
use std::io;
use std::sync::Arc;
use std::sync::mpsc;
use std::thread;
use std::time::{Duration, Instant};

use runinator_api::BlockingServiceLocator;
use runinator_comm::discovery::{WebServiceDiscovery, start_web_service_listener};
use tokio::runtime::Runtime;

#[derive(Serialize, Deserialize, Debug, Clone, Default)]
struct ScheduledTask {
    id: Option<i64>,
    name: String,
    cron_schedule: String,
    action_name: String,
    action_function: String,
    action_configuration: String,
    timeout: i64,
    next_execution: Option<DateTime<Utc>>,
    enabled: bool,
    immediate: bool,
    blackout_start: Option<DateTime<Utc>>,
    blackout_end: Option<DateTime<Utc>>,
}

impl From<ApiScheduledTask> for ScheduledTask {
    fn from(task: ApiScheduledTask) -> Self {
        ScheduledTask {
            id: task.id,
            name: task.name,
            cron_schedule: task.cron_schedule,
            action_name: task.action_name,
            action_function: task.action_function,
            action_configuration: task.action_configuration,
            timeout: task.timeout,
            next_execution: task.next_execution,
            enabled: task.enabled,
            immediate: task.immediate,
            blackout_start: task.blackout_start,
            blackout_end: task.blackout_end,
        }
    }
}

impl ScheduledTask {
    fn to_api(&self) -> ApiScheduledTask {
        ApiScheduledTask {
            id: self.id,
            name: self.name.clone(),
            cron_schedule: self.cron_schedule.clone(),
            action_name: self.action_name.clone(),
            action_function: self.action_function.clone(),
            action_configuration: self.action_configuration.clone(),
            timeout: self.timeout,
            next_execution: self.next_execution.clone(),
            enabled: self.enabled,
            immediate: self.immediate,
            blackout_start: self.blackout_start.clone(),
            blackout_end: self.blackout_end.clone(),
        }
    }
}

type ApiClient = BlockingApiClient<GossipLocator>;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Mode {
    List,
    Editor { creating: bool },
}

impl Default for Mode {
    fn default() -> Self {
        Mode::List
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum MenuKind {
    File,
    Edit,
}

impl Default for MenuKind {
    fn default() -> Self {
        MenuKind::File
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ListFocus {
    Task,
    EditButton,
}

impl Default for ListFocus {
    fn default() -> Self {
        ListFocus::Task
    }
}

#[derive(Default)]
struct AppState {
    // Data
    tasks: Vec<ScheduledTask>,
    selected: usize,
    list_focus: ListFocus,

    // Status
    status: String,
    error: String,
    loading: bool,
    last_refresh: Option<Instant>,

    // Status bar state
    op_in_progress: bool,
    op_label: String,
    spinner_idx: usize,
    last_status_at: Option<Instant>,

    // Menu state
    menu_open: bool,
    active_menu: MenuKind,
    menu_index: usize,

    // Editor state
    mode: Mode,
    editor_draft: ScheduledTask,
    editor_focus: usize, // index of field
    editor_error: String,
    editor_dirty: bool,

    // Quit flag (for menu Quit action)
    should_quit: bool,
}

enum Msg {
    TasksLoaded(Result<Vec<ScheduledTask>, String>),
    RunNowDone(Result<String, String>),
    AddTaskDone(Result<String, String>),
    UpdateTaskDone(Result<String, String>),
}

fn main() -> Result<()> {
    // Initialize terminal
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    stdout.execute(EnterAlternateScreen)?;

    let backend = CrosstermBackend::new(io::stdout());
    let mut terminal = Terminal::new(backend)?;

    // Channel for background -> UI messages
    let (tx, rx) = mpsc::channel::<Msg>();

    let api = ApiClient::new(GossipLocator::from_env()?)?;

    // App state
    let mut app = AppState::default();
    app.mode = Mode::List;
    app.active_menu = MenuKind::File;

    // Initial fetch
    trigger_refresh(&mut app, tx.clone(), api.clone());

    // Timers
    let tick_rate = Duration::from_millis(100);
    let mut last_tick = Instant::now();
    let mut last_auto = Instant::now();
    let auto_refresh = Duration::from_secs(10);

    // Input + render loop
    loop {
        // Draw UI
        terminal.draw(|f| draw_ui(f, &mut app))?;
        if app.should_quit {
            break;
        }

        // Handle messages from workers (non-blocking)
        while let Ok(msg) = rx.try_recv() {
            match msg {
                Msg::TasksLoaded(res) => match res {
                    Ok(tasks) => {
                        app.tasks = tasks;
                        if app.selected >= app.tasks.len() {
                            app.selected = app.tasks.len().saturating_sub(1);
                        }
                        app.error.clear();
                        app.loading = false;
                        app.op_in_progress = false;
                        app.status = "Refreshed.".to_string();
                        app.last_status_at = Some(Instant::now());
                        app.last_refresh = Some(Instant::now());
                    }
                    Err(err) => {
                        app.error = err;
                        app.loading = false;
                        app.op_in_progress = false;
                        app.last_status_at = Some(Instant::now());
                    }
                },
                Msg::RunNowDone(res) => match res {
                    Ok(status) => {
                        app.status = status;
                        app.error.clear();
                        app.op_in_progress = false;
                        app.last_status_at = Some(Instant::now());
                        // Trigger refresh after run
                        trigger_refresh(&mut app, tx.clone(), api.clone());
                    }
                    Err(err) => {
                        app.error = err;
                        app.op_in_progress = false;
                        app.last_status_at = Some(Instant::now());
                    }
                },
                Msg::AddTaskDone(res) => match res {
                    Ok(msg_txt) => {
                        app.status = format!("✔ {}", msg_txt);
                        app.error.clear();
                        app.op_in_progress = false;
                        app.last_status_at = Some(Instant::now());
                        app.mode = Mode::List;
                        trigger_refresh(&mut app, tx.clone(), api.clone());
                    }
                    Err(err) => {
                        app.editor_error = err.clone();
                        app.error = err;
                        app.op_in_progress = false;
                        app.last_status_at = Some(Instant::now());
                    }
                },
                Msg::UpdateTaskDone(res) => match res {
                    Ok(msg_txt) => {
                        app.status = format!("✔ {}", msg_txt);
                        app.error.clear();
                        app.op_in_progress = false;
                        app.last_status_at = Some(Instant::now());
                        app.mode = Mode::List;
                        trigger_refresh(&mut app, tx.clone(), api.clone());
                    }
                    Err(err) => {
                        app.editor_error = err.clone();
                        app.error = err;
                        app.op_in_progress = false;
                        app.last_status_at = Some(Instant::now());
                    }
                },
            }
        }

        // Auto-refresh every 10s
        if last_auto.elapsed() >= auto_refresh && !app.loading && matches!(app.mode, Mode::List) {
            trigger_refresh(&mut app, tx.clone(), api.clone());
            last_auto = Instant::now();
        }

        // Auto-clear success/info status messages after 5s
        if !app.op_in_progress {
            if let Some(t0) = app.last_status_at {
                if t0.elapsed() >= Duration::from_secs(5) {
                    app.status.clear();
                    app.last_status_at = None;
                }
            }
        }

        // Input handling with timeout till next tick
        let timeout = tick_rate
            .checked_sub(last_tick.elapsed())
            .unwrap_or_else(|| Duration::from_millis(0));

        if event::poll(timeout)? {
            if let CEvent::Key(key) = event::read()? {
                if key.kind == KeyEventKind::Press {
                    if handle_key(key, &mut app, tx.clone(), api.clone())? {
                        break;
                    }
                }
            }
        }

        if last_tick.elapsed() >= tick_rate {
            last_tick = Instant::now();
            if app.op_in_progress || app.loading {
                app.spinner_idx = app.spinner_idx.wrapping_add(1);
            }
        }
    }

    // Restore terminal
    disable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, LeaveAlternateScreen)?;
    Ok(())
}

fn handle_key(
    key: KeyEvent,
    app: &mut AppState,
    tx: mpsc::Sender<Msg>,
    api: ApiClient,
) -> Result<bool> {
    // Returns true to quit
    match app.mode {
        Mode::List => handle_list_key(key, app, tx, api),
        Mode::Editor { .. } => handle_editor_key(key, app, tx, api),
    }
}

fn handle_list_key(
    key: KeyEvent,
    app: &mut AppState,
    tx: mpsc::Sender<Msg>,
    api: ApiClient,
) -> Result<bool> {
    let quit = match key.code {
        KeyCode::Char('q') | KeyCode::Esc => true,
        _ => false,
    };

    if quit {
        return Ok(true);
    }

    // Direct shortcuts
    if key.modifiers.contains(KeyModifiers::CONTROL) && key.code == KeyCode::Char('r') {
        trigger_refresh(app, tx, api);
        return Ok(false);
    }
    if key.modifiers.contains(KeyModifiers::CONTROL) && key.code == KeyCode::Char('n') {
        open_editor_new(app);
        return Ok(false);
    }

    // Alt shortcuts to open menus
    if key.modifiers.contains(KeyModifiers::ALT) && matches!(key.code, KeyCode::Char('f')) {
        app.menu_open = true;
        app.active_menu = MenuKind::File;
        app.menu_index = 0;
        return Ok(false);
    }
    if key.modifiers.contains(KeyModifiers::ALT) && matches!(key.code, KeyCode::Char('e')) {
        app.menu_open = true;
        app.active_menu = MenuKind::Edit;
        app.menu_index = 0;
        return Ok(false);
    }

    // Menu navigation
    if app.menu_open {
        match key.code {
            KeyCode::Left => {
                app.active_menu = match app.active_menu {
                    MenuKind::File => MenuKind::Edit,
                    MenuKind::Edit => MenuKind::File,
                }
            }
            KeyCode::Right => {
                app.active_menu = match app.active_menu {
                    MenuKind::File => MenuKind::Edit,
                    MenuKind::Edit => MenuKind::File,
                }
            }
            KeyCode::Up => {
                app.menu_index = app.menu_index.saturating_sub(1);
            }
            KeyCode::Down => {
                app.menu_index = (app.menu_index + 1).min(menu_items(app).len().saturating_sub(1));
            }
            KeyCode::Esc => {
                app.menu_open = false;
            }
            KeyCode::Enter => {
                trigger_menu_action(app, tx, api);
            }
            _ => {}
        }
        return Ok(false);
    }

    // List navigation / actions
    match key.code {
        KeyCode::Up => {
            if app.selected > 0 {
                app.selected -= 1;
                app.list_focus = ListFocus::Task;
            }
        }
        KeyCode::Down => {
            if !app.tasks.is_empty() {
                app.selected = min(app.selected + 1, app.tasks.len() - 1);
                app.list_focus = ListFocus::Task;
            }
        }
        KeyCode::Left => {
            app.list_focus = ListFocus::Task;
        }
        KeyCode::Right => {
            if !app.tasks.is_empty() {
                app.list_focus = ListFocus::EditButton;
            }
        }
        KeyCode::Char('r') => {
            trigger_refresh(app, tx, api);
        }
        KeyCode::Enter => {
            if let Some(task) = app.tasks.get(app.selected) {
                match app.list_focus {
                    ListFocus::Task => {
                        if task.enabled {
                            app.status.clear();
                            app.error.clear();
                            app.op_in_progress = true;
                            app.op_label = format!("Running {}", task.name);
                            spawn_run_now(tx, api, task.id.unwrap_or_default());
                        }
                    }
                    ListFocus::EditButton => {
                        open_editor_existing(app);
                    }
                }
            }
        }
        KeyCode::Char('e') => {
            open_editor_existing(app);
        }
        _ => {}
    }

    Ok(false)
}

fn handle_editor_key(
    key: KeyEvent,
    app: &mut AppState,
    tx: mpsc::Sender<Msg>,
    api: ApiClient,
) -> Result<bool> {
    // Esc closes editor (confirm if dirty?)
    if key.code == KeyCode::Esc {
        app.mode = Mode::List;
        app.list_focus = ListFocus::Task;
        return Ok(false);
    }

    // Tab navigation
    match key.code {
        KeyCode::Tab => {
            app.editor_focus = (app.editor_focus + 1) % editor_field_count();
            return Ok(false);
        }
        KeyCode::BackTab => {
            app.editor_focus = (app.editor_focus + editor_field_count() - 1) % editor_field_count();
            return Ok(false);
        }
        _ => {}
    }

    // Toggle enabled with Space when focused on enabled field
    if app.editor_focus == 6 {
        if let KeyCode::Char(' ') | KeyCode::Enter = key.code {
            app.editor_draft.enabled = !app.editor_draft.enabled;
            app.editor_dirty = true;
            return Ok(false);
        }
    }

    if app.editor_focus == 4
        && matches!(key.code, KeyCode::Enter)
        && key.modifiers.contains(KeyModifiers::SHIFT)
    {
        app.editor_draft.action_configuration.push('\n');
        app.editor_dirty = true;
        return Ok(false);
    }

    // Enter to save when on last field or explicit Ctrl+S
    if key.code == KeyCode::Enter
        || (key.modifiers.contains(KeyModifiers::CONTROL) && matches!(key.code, KeyCode::Char('s')))
    {
        if let Some(err) = validate_editor(&app.editor_draft) {
            app.editor_error = err;
            return Ok(false);
        }
        // Save
        app.status.clear();
        app.error.clear();
        app.op_in_progress = true;
        app.op_label = if matches!(app.mode, Mode::Editor { creating: true }) {
            "Creating task".into()
        } else {
            "Updating task".into()
        };
        if matches!(app.mode, Mode::Editor { creating: true }) {
            spawn_add_task(tx, api, app.editor_draft.clone());
        } else {
            spawn_update_task(tx, api, app.editor_draft.clone());
        }
        return Ok(false);
    }

    // Text input for fields 0..=5
    if let KeyCode::Char(c) = key.code {
        let sref = match app.editor_focus {
            0 => &mut app.editor_draft.name,
            1 => &mut app.editor_draft.cron_schedule,
            2 => &mut app.editor_draft.action_name,
            3 => &mut app.editor_draft.action_function,
            4 => &mut app.editor_draft.action_configuration,
            5 => {
                // timeout numeric
                if c.is_ascii_digit() {
                    let mut cur = app.editor_draft.timeout.to_string();
                    cur.push(c);
                    if let Ok(v) = cur.parse::<i64>() {
                        app.editor_draft.timeout = v;
                        app.editor_dirty = true;
                    }
                }
                return Ok(false);
            }
            _ => {
                return Ok(false);
            }
        };
        sref.push(c);
        app.editor_dirty = true;
        return Ok(false);
    }

    if key.code == KeyCode::Backspace {
        match app.editor_focus {
            0 => {
                app.editor_draft.name.pop();
            }
            1 => {
                app.editor_draft.cron_schedule.pop();
            }
            2 => {
                app.editor_draft.action_name.pop();
            }
            3 => {
                app.editor_draft.action_function.pop();
            }
            4 => {
                app.editor_draft.action_configuration.pop();
            }
            5 => {
                let mut cur = app.editor_draft.timeout.to_string();
                cur.pop();
                app.editor_draft.timeout = cur.parse::<i64>().unwrap_or(0);
            }
            _ => {}
        }
        app.editor_dirty = true;
        return Ok(false);
    }

    Ok(false)
}

fn open_editor_new(app: &mut AppState) {
    app.mode = Mode::Editor { creating: true };
    app.editor_draft = ScheduledTask {
        id: None,
        name: String::new(),
        cron_schedule: String::new(),
        action_name: String::new(),
        action_function: String::new(),
        action_configuration: String::new(),
        timeout: 0,
        next_execution: None,
        enabled: true,
        immediate: false,
        blackout_start: None,
        blackout_end: None,
    };
    app.editor_focus = 0;
    app.editor_dirty = false;
    app.editor_error.clear();
}

fn open_editor_existing(app: &mut AppState) {
    if let Some(task) = app.tasks.get(app.selected).cloned() {
        app.mode = Mode::Editor { creating: false };
        app.editor_draft = task;
        app.editor_focus = 0;
        app.editor_dirty = false;
        app.editor_error.clear();
    } else {
        app.error = "No task selected".into();
        app.last_status_at = Some(Instant::now());
    }
}

fn trigger_menu_action(app: &mut AppState, tx: mpsc::Sender<Msg>, api: ApiClient) {
    let items = menu_items(app);
    let idx = app.menu_index.min(items.len().saturating_sub(1));
    match (app.active_menu, idx) {
        (MenuKind::File, 0) => {
            // Refresh
            trigger_refresh(app, tx, api);
        }
        (MenuKind::File, 1) => {
            // Quit
            app.should_quit = true;
        }
        (MenuKind::Edit, 0) => {
            // Add New Task
            open_editor_new(app);
        }
        (MenuKind::Edit, 1) => {
            // Edit Selected
            open_editor_existing(app);
        }
        _ => {}
    }
    app.menu_open = false;
}

fn menu_items(app: &AppState) -> Vec<&'static str> {
    match app.active_menu {
        MenuKind::File => vec!["Refresh\tCtrl+R", "Quit\tq/Esc"],
        MenuKind::Edit => vec!["Add New Task\tCtrl+N", "Edit Selected\tE"],
    }
}

fn trigger_refresh(app: &mut AppState, tx: mpsc::Sender<Msg>, api: ApiClient) {
    spawn_fetch(tx, api);
    app.loading = true;
    app.status.clear();
    app.error.clear();
    app.op_in_progress = true;
    app.op_label = "Refreshing tasks".to_string();
}

fn spawn_fetch(tx: mpsc::Sender<Msg>, api: ApiClient) {
    thread::spawn(move || {
        let res = api
            .fetch_tasks()
            .map(|tasks| tasks.into_iter().map(ScheduledTask::from).collect())
            .map_err(|e| e.to_string());
        let _ = tx.send(Msg::TasksLoaded(res));
    });
}

fn spawn_run_now(tx: mpsc::Sender<Msg>, api: ApiClient, id: i64) {
    thread::spawn(move || {
        let res = api
            .request_task_run(id)
            .map(|resp| {
                format!(
                    "{}: {}",
                    if resp.success { "OK" } else { "ERR" },
                    resp.message
                )
            })
            .map_err(|e| e.to_string());
        let _ = tx.send(Msg::RunNowDone(res));
    });
}

fn prepare_task_for_submission(task: &mut ScheduledTask) {
    if task.next_execution.is_none() {
        task.next_execution = Some(Utc::now());
    }
}

fn spawn_add_task(tx: mpsc::Sender<Msg>, api: ApiClient, mut task: ScheduledTask) {
    prepare_task_for_submission(&mut task);
    thread::spawn(move || {
        let payload = task.to_api();
        let res = api
            .upsert_task(&payload)
            .map(|resp| {
                format!(
                    "{}: {}",
                    if resp.success { "OK" } else { "ERR" },
                    resp.message
                )
            })
            .map_err(|e| e.to_string());
        let _ = tx.send(Msg::AddTaskDone(res));
    });
}

fn spawn_update_task(tx: mpsc::Sender<Msg>, api: ApiClient, mut task: ScheduledTask) {
    prepare_task_for_submission(&mut task);
    thread::spawn(move || {
        let res = if task.id.is_none() {
            Err("Task is missing an ID".to_string())
        } else {
            let payload = task.to_api();
            api.update_task(&payload)
                .map(|resp| {
                    format!(
                        "{}: {}",
                        if resp.success { "OK" } else { "ERR" },
                        resp.message
                    )
                })
                .map_err(|e| e.to_string())
        };
        let _ = tx.send(Msg::UpdateTaskDone(res));
    });
}

fn draw_ui(frame: &mut ratatui::Frame, app: &mut AppState) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3), // header/menu bar
            Constraint::Min(3),    // list or editor
            Constraint::Length(1), // status bar
        ])
        .split(frame.size());

    draw_menubar(frame, chunks[0], app);

    match app.mode {
        Mode::List => draw_task_list(frame, chunks[1], app),
        Mode::Editor { .. } => draw_editor(frame, chunks[1], app),
    }

    // Status bar (single line, no borders)
    let spinner_frames = ["-", "\\", "|", "/"]; // ASCII-safe spinner
    let mut line = "Ready.".to_string();
    let mut style = Style::default().fg(Color::DarkGray);

    if !app.error.is_empty() {
        line = format!("Error: {}", app.error);
        style = Style::default().fg(Color::Red);
    } else if app.op_in_progress || app.loading {
        let framec = spinner_frames[app.spinner_idx % spinner_frames.len()];
        let label = if app.op_label.is_empty() {
            "Working"
        } else {
            &app.op_label
        };
        line = format!(" {} {}...", framec, label);
        style = Style::default().fg(Color::Yellow);
    } else if !app.status.is_empty() {
        line = format!("✔ {}", app.status);
        style = Style::default().fg(Color::Green);
    }

    let status_bar = Paragraph::new(Line::from(Span::styled(line, style)));
    frame.render_widget(status_bar, chunks[2]);
}

fn draw_menubar(frame: &mut ratatui::Frame, area: Rect, app: &AppState) {
    // Render a simple menubar with File and Edit
    let titles = vec![
        (" File ", matches!(app.active_menu, MenuKind::File)),
        (" Edit ", matches!(app.active_menu, MenuKind::Edit)),
    ];

    let mut line = Line::default();
    for (idx, (title, active)) in titles.iter().enumerate() {
        let mut style = Style::default().fg(Color::White);
        if *active {
            style = style.add_modifier(Modifier::BOLD).fg(Color::Yellow);
        }
        line.spans.push(Span::styled(*title, style));
        if idx == 0 {
            line.spans.push(Span::raw("  "));
        }
    }

    let bar = Paragraph::new(line)
        .block(
            Block::default()
                .title("Command Center")
                .borders(Borders::ALL),
        )
        .alignment(Alignment::Left);
    frame.render_widget(bar, area);

    if app.menu_open {
        // Draw dropdown
        let items = menu_items(app);
        let menu_width = items.iter().map(|s| s.len()).max().unwrap_or(10) as u16 + 4;
        let x = match app.active_menu {
            MenuKind::File => area.x + 2,
            MenuKind::Edit => area.x + 10,
        };
        let y = area.y + 1;
        let height = (items.len() as u16).max(1) + 2;
        let popup = Rect {
            x,
            y,
            width: menu_width,
            height,
        };

        let list_items: Vec<ListItem> = items.iter().map(|s| ListItem::new(*s)).collect();
        let mut state = ListState::default();
        state.select(Some(app.menu_index.min(items.len().saturating_sub(1))));
        let list = List::new(list_items)
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .title(match app.active_menu {
                        MenuKind::File => "File",
                        MenuKind::Edit => "Edit",
                    }),
            )
            .highlight_style(Style::default().add_modifier(Modifier::REVERSED));

        frame.render_widget(Clear, popup); // clears out the background
        frame.render_stateful_widget(list, popup, &mut state);
    }
}

fn draw_task_list(frame: &mut ratatui::Frame, area: Rect, app: &mut AppState) {
    if app.tasks.is_empty() {
        let message = if app.loading {
            "Loading tasks..."
        } else if !app.error.is_empty() {
            app.error.as_str()
        } else {
            "No tasks found."
        };
        let placeholder = Paragraph::new(Line::from(Span::raw(message)))
            .block(
                Block::default()
                    .title("Scheduled Tasks")
                    .borders(Borders::ALL),
            )
            .alignment(Alignment::Center);
        frame.render_widget(placeholder, area);
        return;
    }

    let header_cells = ["Name", "Cron", "Next Run", "Enabled", "Timeout", "Action"]
        .into_iter()
        .map(|h| Cell::from(h).style(Style::default().add_modifier(Modifier::BOLD)));
    let header = Row::new(header_cells).style(Style::default().fg(Color::Cyan));

    let rows: Vec<Row> = app
        .tasks
        .iter()
        .enumerate()
        .map(|(idx, task)| {
            let name_span: Span = if task.enabled {
                Span::raw(task.name.clone())
            } else {
                Span::styled(task.name.clone(), Style::default().fg(Color::DarkGray))
            };
            let action_span =
                if idx == app.selected && matches!(app.list_focus, ListFocus::EditButton) {
                    Span::styled(
                        "[Edit]",
                        Style::default()
                            .fg(Color::Black)
                            .bg(Color::Yellow)
                            .add_modifier(Modifier::BOLD),
                    )
                } else {
                    Span::styled("[Edit]", Style::default().fg(Color::Yellow))
                };

            let mut row = Row::new(vec![
                Cell::from(name_span),
                Cell::from(task.cron_schedule.clone()),
                Cell::from(format_next_execution(task.next_execution.as_ref())),
                Cell::from(if task.enabled { "Yes" } else { "No" }),
                Cell::from(format!("{} ms", task.timeout)),
                Cell::from(action_span),
            ]);

            if !task.enabled {
                row = row.style(Style::default().fg(Color::DarkGray));
            }

            row
        })
        .collect();

    let mut table_state = TableState::default();
    table_state.select(Some(app.selected));

    let widths = [
        Constraint::Percentage(25),
        Constraint::Percentage(20),
        Constraint::Percentage(25),
        Constraint::Length(8),
        Constraint::Length(12),
        Constraint::Length(8),
    ];

    let table = Table::new(rows, widths)
        .header(header)
        .block(
            Block::default()
                .title("Scheduled Tasks")
                .borders(Borders::ALL),
        )
        .column_spacing(1)
        .highlight_style(Style::default().add_modifier(Modifier::REVERSED))
        .highlight_symbol("");

    frame.render_stateful_widget(table, area, &mut table_state);
}

fn format_next_execution(dt: Option<&DateTime<Utc>>) -> String {
    dt.map(|val| val.format("%Y-%m-%d %H:%M:%S").to_string())
        .unwrap_or_else(|| "-".into())
}

fn draw_editor(frame: &mut ratatui::Frame, area: Rect, app: &AppState) {
    // Render a simple form with fields 0..=6
    let labels = [
        "Name:",
        "Cron:",
        "Action Name:",
        "Action Function:",
        "Action Config:",
        "Timeout (ms):",
        "Enabled:",
    ];

    let mut text = Text::from("");
    for (i, label) in labels.iter().enumerate() {
        let label_style = if i == app.editor_focus {
            Style::default()
                .fg(Color::Yellow)
                .add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(Color::White)
        };
        text.extend(Text::from(Line::from(vec![Span::styled(
            *label,
            label_style,
        )])));
        match i {
            0 => text.extend(Text::from(Line::from(vec![Span::raw(format!(
                "  {}",
                app.editor_draft.name
            ))]))),
            1 => text.extend(Text::from(Line::from(vec![Span::raw(format!(
                "  {}",
                app.editor_draft.cron_schedule
            ))]))),
            2 => text.extend(Text::from(Line::from(vec![Span::raw(format!(
                "  {}",
                app.editor_draft.action_name
            ))]))),
            3 => text.extend(Text::from(Line::from(vec![Span::raw(format!(
                "  {}",
                app.editor_draft.action_function
            ))]))),
            4 => {
                if app.editor_draft.action_configuration.is_empty() {
                    text.extend(Text::from(Line::from(vec![Span::styled(
                        "  <empty>",
                        Style::default().fg(Color::DarkGray),
                    )])));
                } else {
                    for line in app.editor_draft.action_configuration.split('\n') {
                        text.extend(Text::from(Line::from(vec![Span::raw(format!(
                            "  {}",
                            line
                        ))])));
                    }
                }
                if i == app.editor_focus {
                    text.extend(Text::from(Line::from(vec![Span::styled(
                        "  Shift+Enter for newline",
                        Style::default().fg(Color::DarkGray),
                    )])));
                }
            }
            5 => text.extend(Text::from(Line::from(vec![Span::raw(format!(
                "  {}",
                app.editor_draft.timeout
            ))]))),
            6 => {
                let value = if app.editor_draft.enabled {
                    "  Yes (Space to toggle)"
                } else {
                    "  No (Space to toggle)"
                };
                text.extend(Text::from(Line::from(vec![Span::raw(value)])));
            }
            _ => {}
        }
        text.extend(Text::from(Line::from("")));
    }

    if !app.editor_error.is_empty() {
        text.extend(Text::from(Line::from(vec![Span::styled(
            format!("Error: {}", app.editor_error),
            Style::default().fg(Color::Red),
        )])));
    }
    text.extend(Text::from(Line::from("")));
    text.extend(Text::from(Line::from(
        "Enter/Ctrl+S: Save   Esc: Cancel   Tab/Shift+Tab: Move   Shift+Enter: New line (Config)",
    )));

    let block = Block::default()
        .title(match app.mode {
            Mode::Editor { creating: true } => "New Task",
            _ => "Edit Task",
        })
        .borders(Borders::ALL);

    let para = Paragraph::new(text).block(block).wrap(Wrap { trim: false });
    frame.render_widget(para, area);
}

fn validate_editor(task: &ScheduledTask) -> Option<String> {
    if task.name.trim().is_empty() {
        return Some("Name is required".into());
    }
    if task.cron_schedule.trim().is_empty() {
        return Some("Cron is required".into());
    }
    if task.action_name.trim().is_empty() {
        return Some("Action name is required".into());
    }
    if task.action_function.trim().is_empty() {
        return Some("Action function is required".into());
    }
    if task.action_configuration.trim().is_empty() {
        return Some("Action configuration is required".into());
    }
    if task.timeout <= 0 {
        return Some("Timeout must be > 0".into());
    }
    None
}

fn editor_field_count() -> usize {
    7
}
#[derive(Clone)]
struct GossipLocator {
    discovery: WebServiceDiscovery,
    runtime: Arc<Runtime>,
}

impl GossipLocator {
    pub fn from_env() -> Result<Self> {
        let bind =
            std::env::var("RUNINATOR_GOSSIP_BIND").unwrap_or_else(|_| "127.0.0.1".to_string());
        let port = std::env::var("RUNINATOR_GOSSIP_PORT")
            .unwrap_or_else(|_| "5504".to_string())
            .parse::<u16>()
            .unwrap_or(5000);

        Self::new(bind, port)
    }

    pub fn new(gossip_bind: String, gossip_port: u16) -> Result<Self> {
        let runtime = Arc::new(Runtime::new()?);
        let discovery = runtime.block_on(start_web_service_listener(
            gossip_bind.as_str(),
            gossip_port,
        ))?;
        Ok(Self { discovery, runtime })
    }
}

impl BlockingServiceLocator for GossipLocator {
    type Error = std::convert::Infallible;

    fn wait_for_service_url(&self) -> Result<String, Self::Error> {
        Ok(self.runtime.block_on(self.discovery.wait_for_service_url()))
    }
}
