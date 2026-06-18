use std::path::PathBuf;
use uuid::Uuid;

use clap::{Parser, Subcommand, ValueEnum};
use runinator_models::semver::SemVerBump;
use runinator_models::settings::SettingKind;
use runinator_wdl::TypePolicy;

/// cli-facing semantic-version bump level, mapped to the shared `SemVerBump`.
#[derive(Debug, Clone, Copy, Default, ValueEnum)]
pub enum CliBumpLevel {
    Major,
    #[default]
    Minor,
    Patch,
}

impl From<CliBumpLevel> for SemVerBump {
    fn from(level: CliBumpLevel) -> Self {
        match level {
            CliBumpLevel::Major => SemVerBump::Major,
            CliBumpLevel::Minor => SemVerBump::Minor,
            CliBumpLevel::Patch => SemVerBump::Patch,
        }
    }
}

/// cli-facing setting kind, mapped to the shared `SettingKind`.
#[derive(Debug, Clone, Copy, Default, ValueEnum)]
pub enum CliSettingKind {
    #[default]
    Secret,
    Config,
}

impl From<CliSettingKind> for SettingKind {
    fn from(kind: CliSettingKind) -> Self {
        match kind {
            CliSettingKind::Secret => SettingKind::Secret,
            CliSettingKind::Config => SettingKind::Config,
        }
    }
}

/// cli-facing WDL typing policy.
#[derive(Debug, Clone, Copy, Default, ValueEnum)]
pub enum CliTyping {
    #[default]
    Strict,
    Permissive,
}

impl From<CliTyping> for TypePolicy {
    fn from(policy: CliTyping) -> Self {
        match policy {
            CliTyping::Strict => TypePolicy::Strict,
            CliTyping::Permissive => TypePolicy::Permissive,
        }
    }
}

#[derive(Debug, Parser)]
#[command(
    name = "runinatorctl",
    about = "Control Runinator from the command line"
)]
pub struct Cli {
    #[arg(
        long,
        global = true,
        env = "RUNINATOR_API_BASE_URL",
        default_value = "http://127.0.0.1:8080/"
    )]
    pub api_base_url: String,

    /// API key or access token presented as `Authorization: Bearer …` (needed when the web
    /// service has auth enabled).
    #[arg(long, global = true, env = "RUNINATOR_API_KEY")]
    pub api_key: Option<String>,

    #[arg(long, global = true)]
    pub json: bool,

    #[command(subcommand)]
    pub command: Commands,
}

#[derive(Debug, Subcommand)]
pub enum Commands {
    /// Store a local authenticated session for auth-enabled servers.
    Login {
        #[arg(long)]
        username: Option<String>,
        #[arg(long)]
        password: Option<String>,
    },
    /// Revoke the stored session for the selected server and remove it locally.
    Logout,
    /// Show API, supervisor, and active-run health.
    Status,
    /// Inspect and run workflow definitions.
    Workflows {
        #[command(subcommand)]
        command: WorkflowCommands,
    },
    /// Inspect and control workflow runs.
    Runs {
        #[command(subcommand)]
        command: RunCommands,
    },
    /// Inspect and resolve approval requests.
    Approvals {
        #[command(subcommand)]
        command: ApprovalCommands,
    },
    /// Inspect workflow triggers.
    Triggers {
        #[command(subcommand)]
        command: TriggerCommands,
    },
    /// Inspect provider/action metadata.
    Providers {
        #[command(subcommand)]
        command: ProviderCommands,
    },
    /// List and download run artifacts.
    Artifacts {
        #[command(subcommand)]
        command: ArtifactCommands,
    },
    /// Compile, decompile, format, and check the wdl workflow language.
    Wdl {
        #[command(subcommand)]
        command: WdlCommands,
    },
    /// Manage the unified settings store: secrets and config.
    Settings {
        #[command(subcommand)]
        command: SettingsCommands,
    },
}

#[derive(Debug, Subcommand)]
pub enum SettingsCommands {
    /// List stored settings (secrets and config) without their values.
    List {
        /// Only show one kind.
        #[arg(long, value_enum)]
        kind: Option<CliSettingKind>,
    },
    /// Get a setting value. Config returns json; secrets return the stored string.
    Get {
        scope: String,
        name: String,
        #[arg(long, value_enum, default_value_t = CliSettingKind::Secret)]
        kind: CliSettingKind,
    },
    /// Store a setting value. Provide VALUE inline or read it from --value-file. For config,
    /// the value is parsed as json and validated against the schema (required once per slot via
    /// --schema; reused on later updates); for secrets the value is stored verbatim.
    Set {
        scope: String,
        name: String,
        /// inline value; omit when reading from --value-file.
        value: Option<String>,
        /// read the value from a file instead of the VALUE argument.
        #[arg(long, value_name = "PATH", conflicts_with = "value")]
        value_file: Option<PathBuf>,
        #[arg(long, value_enum, default_value_t = CliSettingKind::Secret)]
        kind: CliSettingKind,
        /// JSON-schema for a config value (json text), required on first write of a config slot.
        #[arg(long)]
        schema: Option<String>,
    },
    /// Import settings from a `.wdls` secrets file (`secret|config <scope>.<name> = <literal>`
    /// declarations). JSON is not accepted.
    Import { file: PathBuf },
    /// Delete a setting.
    Delete {
        scope: String,
        name: String,
        #[arg(long, value_enum, default_value_t = CliSettingKind::Secret)]
        kind: CliSettingKind,
    },
}

#[derive(Debug, Subcommand)]
pub enum WdlCommands {
    /// Compile a .wdl file into a workflow definition JSON.
    Compile {
        file: PathBuf,
        #[arg(short, long)]
        output: Option<PathBuf>,
        /// WDL type checking policy. Use permissive only for legacy investigation.
        #[arg(long, value_enum, default_value_t = CliTyping::Strict)]
        typing: CliTyping,
    },
    /// Decompile a workflow definition JSON file back into .wdl source.
    Decompile {
        file: PathBuf,
        #[arg(short, long)]
        output: Option<PathBuf>,
        /// Emit the canonical fully-explicit form: start edge, ids and arrows on every node,
        /// and all defaulted values (timeout/retry/limit/concurrency/approval type).
        #[arg(long)]
        explicit: bool,
    },
    /// Format a .wdl file.
    Format {
        file: PathBuf,
        #[arg(short, long)]
        output: Option<PathBuf>,
        #[arg(long)]
        check: bool,
    },
    /// Parse, lower, and validate a .wdl file, printing any diagnostics.
    Check {
        file: PathBuf,
        /// WDL type checking policy. Use permissive only for legacy investigation.
        #[arg(long, value_enum, default_value_t = CliTyping::Strict)]
        typing: CliTyping,
    },
}

#[derive(Debug, Subcommand)]
pub enum WorkflowCommands {
    /// List workflow definitions.
    List,
    /// Show a workflow by id or name.
    Show { workflow: String },
    /// Validate a workflow definition JSON file.
    Validate { file: PathBuf },
    /// Import a workflow pack (.wdl, .wdlp, or a directory of .wdl files), or save a workflow
    /// definition / import a workflow bundle from a JSON file. For a pack, an adjacent settings
    /// bundle (a `.wdlp` "settings" entry or a sibling settings.wdls/settings.json) is always
    /// imported too to seed config/secret slots. When no path is given, falls back to the
    /// `~/.runinator/workflows` folder if it exists.
    Apply { file: Option<PathBuf> },
    /// Watch a workflow pack, re-apply it on changes, and optionally run a workflow.
    Dev {
        file: Option<PathBuf>,
        /// Workflow id or name to run after each successful apply.
        #[arg(long)]
        run: Option<String>,
        #[arg(long = "param", value_name = "KEY=VALUE")]
        params: Vec<String>,
        #[arg(long = "json-file")]
        json_file: Option<PathBuf>,
        #[arg(long)]
        debug: bool,
        /// Name assigned to each created workflow run.
        #[arg(long)]
        name: Option<String>,
        /// How often to check source mtimes.
        #[arg(long, default_value_t = 500)]
        watch_interval_ms: u64,
        /// Quiet period after a change before compiling/importing.
        #[arg(long, default_value_t = 250)]
        debounce_ms: u64,
    },
    /// Export one workflow or the full workflow bundle.
    Export {
        workflow_id: Option<Uuid>,
        #[arg(short, long)]
        output: Option<PathBuf>,
    },
    /// Duplicate a workflow into a new version sharing its name (default bump: minor).
    Duplicate {
        /// Workflow id or name to duplicate.
        workflow: String,
        #[arg(long, value_enum, default_value_t = CliBumpLevel::default())]
        bump: CliBumpLevel,
    },
    /// Create a workflow run.
    Run {
        workflow: String,
        #[arg(long = "param", value_name = "KEY=VALUE")]
        params: Vec<String>,
        #[arg(long = "json-file")]
        json_file: Option<PathBuf>,
        #[arg(long)]
        debug: bool,
        #[arg(long)]
        name: Option<String>,
    },
}

#[derive(Debug, Subcommand)]
pub enum RunCommands {
    /// List recent or filtered workflow runs.
    List {
        #[arg(long)]
        status: Option<String>,
        #[arg(long = "workflow-id")]
        workflow_id: Option<Uuid>,
        #[arg(long)]
        open: bool,
    },
    /// Show a workflow run and its node runs.
    Show { id: Uuid },
    /// Refresh a workflow run until interrupted or terminal.
    Watch {
        id: Uuid,
        #[arg(long, default_value_t = 2)]
        interval_seconds: u64,
    },
    /// Print log chunks for a workflow node run.
    Logs {
        node_run_id: Uuid,
        #[arg(long)]
        cursor: Option<i64>,
        #[arg(long, default_value_t = 100)]
        limit: i64,
    },
    /// Pause a workflow run.
    Pause { id: Uuid },
    /// Resume a workflow run.
    Resume { id: Uuid },
    /// Cancel a workflow run.
    Cancel { id: Uuid },
    /// Replay a workflow run.
    Replay {
        id: Uuid,
        #[arg(long = "from-step")]
        from_step_id: Option<String>,
    },
    /// Rename a workflow run.
    Rename { id: Uuid, name: Option<String> },
    /// List the run-level deliverables a workflow run produced.
    Deliverables { id: Uuid },
}

#[derive(Debug, Subcommand)]
pub enum ArtifactCommands {
    /// List the artifacts a workflow node run produced.
    List {
        #[arg(long = "node-run")]
        node_run_id: Uuid,
    },
    /// Download an artifact by id to a file (defaults to its name in the cwd).
    Download {
        id: Uuid,
        #[arg(short, long)]
        out: Option<PathBuf>,
    },
}

#[derive(Debug, Subcommand)]
pub enum ApprovalCommands {
    /// List approval requests.
    List {
        #[arg(long = "workflow-run-id")]
        workflow_run_id: Option<Uuid>,
        #[arg(long)]
        open: bool,
    },
    /// Approve an approval request.
    Approve {
        id: Uuid,
        #[arg(long)]
        by: Option<String>,
        #[arg(long)]
        message: Option<String>,
        #[arg(long = "json-file")]
        json_file: Option<PathBuf>,
    },
    /// Reject an approval request.
    Reject {
        id: Uuid,
        #[arg(long)]
        by: Option<String>,
        #[arg(long)]
        message: Option<String>,
        #[arg(long = "json-file")]
        json_file: Option<PathBuf>,
    },
}

#[derive(Debug, Subcommand)]
pub enum TriggerCommands {
    /// List triggers for a workflow by id or name.
    List { workflow: String },
    /// List triggers due for execution.
    Due,
    /// Create a run from a trigger.
    Run {
        trigger_id: Uuid,
        #[arg(long = "param", value_name = "KEY=VALUE")]
        params: Vec<String>,
        #[arg(long = "json-file")]
        json_file: Option<PathBuf>,
        #[arg(long)]
        debug: bool,
    },
}

#[derive(Debug, Subcommand)]
pub enum ProviderCommands {
    /// List providers.
    List,
    /// Show one provider by name.
    Show { name: String },
}
