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

mod operations_screen;
use operations_screen::OperationsScreen;

mod state;
use state::State;
