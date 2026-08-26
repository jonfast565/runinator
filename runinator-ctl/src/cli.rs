use std::path::PathBuf;
use uuid::Uuid;

use chrono::{DateTime, Utc};
use clap::{Parser, Subcommand, ValueEnum};
use runinator_models::provisioning::ProvisionBackend;
use runinator_models::replicas::{ReplicaKind, ReplicaStatus};
use runinator_models::semver::SemVerBump;
use runinator_models::settings::SettingKind;
use runinator_rexrap::TypePolicy;

/// CLI-facing semantic-version bump level, mapped to the shared `SemVerBump`.
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

/// CLI-facing setting kind, mapped to the shared `SettingKind`.
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

/// CLI-facing REXRAP typing policy.
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

/// CLI-facing provisioning backend, mapped to the shared `ProvisionBackend`.
#[derive(Debug, Clone, Copy, ValueEnum)]
pub enum CliProvisionBackend {
    Supervisor,
    Kubernetes,
}

impl From<CliProvisionBackend> for ProvisionBackend {
    fn from(backend: CliProvisionBackend) -> Self {
        match backend {
            CliProvisionBackend::Supervisor => ProvisionBackend::Supervisor,
            CliProvisionBackend::Kubernetes => ProvisionBackend::Kubernetes,
        }
    }
}

/// CLI-facing node kind, mapped to the shared `ReplicaKind`.
#[derive(Debug, Clone, Copy, ValueEnum)]
pub enum CliNodeKind {
    Worker,
    Waker,
    Webservice,
    Postgres,
}

impl From<CliNodeKind> for ReplicaKind {
    fn from(kind: CliNodeKind) -> Self {
        match kind {
            CliNodeKind::Worker => ReplicaKind::Worker,
            CliNodeKind::Waker => ReplicaKind::Waker,
            CliNodeKind::Webservice => ReplicaKind::Webservice,
            CliNodeKind::Postgres => ReplicaKind::Postgres,
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

    /// Username to sign in with when the server enforces auth and no session is stored yet.
    #[arg(long, global = true, env = "RUNINATOR_USERNAME")]
    pub username: Option<String>,

    /// Password for `--username`. Prefer the environment variable so it stays out of shell
    /// history and process listings.
    #[arg(
        long,
        global = true,
        env = "RUNINATOR_PASSWORD",
        hide_env_values = true
    )]
    pub password: Option<String>,

    #[arg(long, global = true)]
    pub json: bool,

    #[command(subcommand)]
    pub command: Commands,
}

#[derive(Debug, Subcommand)]
pub enum Commands {
    /// Store a local authenticated session for auth-enabled servers. Credentials come from the
    /// global `--username`/`--password` options, and are prompted for when absent.
    Login,
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
    /// Manage freeze windows that suspend trigger firing.
    Freeze {
        #[command(subcommand)]
        command: FreezeCommands,
    },
    /// Publish and promote packaged functions.
    Functions {
        #[command(subcommand)]
        command: FunctionCommands,
    },
    /// Inspect and run pipelines. Pipeline *shape* is pack-managed (an `.rrx` pipeline block, applied by
    /// `workflows apply`); these verbs read and drive what a pack defined.
    Pipelines {
        #[command(subcommand)]
        command: PipelineCommands,
    },
    /// Serve MCP on stdin/stdout. Expose every runinatorctl command as a tool.
    /// An MCP client should launch this command; it speaks JSON-RPC, so
    /// command output is captured into tool results instead of being printed.
    Mcp {
        /// Also expose every enabled workflow as a tool that starts a run of it. Off by default:
        /// the tool list is context the model pays for on every turn, and a fleet of workflows
        /// would bury the commands that author them.
        #[arg(long)]
        workflow_tools: bool,
        /// Seconds one command may run before its tool call gives up.
        #[arg(long, default_value_t = 300)]
        timeout: u64,
    },
    /// Open a durable, multiline REXRAP execution console.
    Console {
        /// Resume a session by UUID or name.
        #[arg(long)]
        session: Option<String>,
        /// Create and use a new named session.
        #[arg(long = "new")]
        new_session: Option<String>,
        /// Execute one cell and exit.
        #[arg(short = 'e', long)]
        execute: Option<String>,
        /// Execute a REXRAP cell read from a file and exit.
        #[arg(short, long)]
        file: Option<PathBuf>,
        /// Return as soon as an effectful cell has started.
        #[arg(long)]
        no_follow: bool,
        /// Use the plain line editor instead of the terminal UI.
        #[arg(long)]
        plain: bool,
    },
    /// Inspect provider/action metadata.
    Providers {
        #[command(subcommand)]
        command: ProviderCommands,
    },
    /// List artifacts produced by workflow effects.
    Artifacts {
        #[command(subcommand)]
        command: ArtifactCommands,
    },
    /// Compile, decompile, format, and check the rexrap workflow language.
    #[command(name = "rexrap")]
    RexRap {
        #[command(subcommand)]
        command: RexRapCommands,
    },
    /// Manage the unified settings store: secrets and config.
    Settings {
        #[command(subcommand)]
        command: SettingsCommands,
    },
    /// Plan and apply the one-time migration to UUID-backed namespace aliases.
    Namespaces {
        #[command(subcommand)]
        command: NamespaceCommands,
    },
    /// Spin up, scale, and stop worker/waker nodes on demand.
    Nodes {
        #[command(subcommand)]
        command: NodeCommands,
    },
    /// Manage organizations, their resource allocation, and usage/cost.
    Orgs {
        #[command(subcommand)]
        command: OrgCommands,
    },
    /// Inspect the registered runtime replicas behind the fleet.
    Replicas {
        #[command(subcommand)]
        command: ReplicaCommands,
    },
    /// Enroll and manage externally hosted worker agents.
    Agents {
        #[command(subcommand)]
        command: AgentCommands,
    },
}

#[derive(Debug, Subcommand)]
pub enum NamespaceCommands {
    /// Write an editable JSON mapping for workflows, pipelines, function packages, and settings.
    Plan {
        /// Write the mapping to this file; omit to print it.
        #[arg(short, long)]
        output: Option<PathBuf>,
    },
    /// Apply an edited mapping after verifying that all workflow and pipeline runs have drained.
    Apply { file: PathBuf },
}

/// CLI-facing replica kind. wider than `CliNodeKind`, which only names the kinds a provisioner can
/// scale; every kind that registers can be listed.
#[derive(Debug, Clone, Copy, ValueEnum)]
pub enum CliReplicaKind {
    Worker,
    Waker,
    Webservice,
    Background,
    Archiver,
    Postgres,
}

impl From<CliReplicaKind> for ReplicaKind {
    fn from(kind: CliReplicaKind) -> Self {
        match kind {
            CliReplicaKind::Worker => ReplicaKind::Worker,
            CliReplicaKind::Waker => ReplicaKind::Waker,
            CliReplicaKind::Webservice => ReplicaKind::Webservice,
            CliReplicaKind::Background => ReplicaKind::Background,
            CliReplicaKind::Archiver => ReplicaKind::Archiver,
            CliReplicaKind::Postgres => ReplicaKind::Postgres,
        }
    }
}

/// CLI-facing replica status.
#[derive(Debug, Clone, Copy, ValueEnum)]
pub enum CliReplicaStatus {
    Live,
    Stale,
    Offline,
}

impl From<CliReplicaStatus> for ReplicaStatus {
    fn from(status: CliReplicaStatus) -> Self {
        match status {
            CliReplicaStatus::Live => ReplicaStatus::Live,
            CliReplicaStatus::Stale => ReplicaStatus::Stale,
            CliReplicaStatus::Offline => ReplicaStatus::Offline,
        }
    }
}

#[derive(Debug, Subcommand)]
pub enum ReplicaCommands {
    /// List registered replicas and their ids, optionally filtered by kind or status.
    List {
        /// Only replicas of one kind.
        #[arg(long, value_enum)]
        kind: Option<CliReplicaKind>,
        /// Only replicas in one status.
        #[arg(long, value_enum)]
        status: Option<CliReplicaStatus>,
        /// Only show replicas still reporting in.
        #[arg(long)]
        live: bool,
    },
    /// Show one replica by id, including the attributes it heartbeats.
    Show { replica_id: Uuid },
    /// Print just the replica ids, one per line, for piping into another command.
    Ids {
        /// Only replicas of one kind.
        #[arg(long, value_enum)]
        kind: Option<CliReplicaKind>,
        /// Only replicas in one status.
        #[arg(long, value_enum)]
        status: Option<CliReplicaStatus>,
    },
    /// List the providers one replica has registered.
    Providers { replica_id: Uuid },
    /// Show one replica's recent cpu/memory telemetry.
    Samples {
        replica_id: Uuid,
        /// Look-back window in seconds; defaults to the last hour.
        #[arg(long)]
        since_seconds: Option<i64>,
        /// Cap on the samples printed, newest last.
        #[arg(long, default_value_t = 20)]
        limit: usize,
    },
}

#[derive(Debug, Subcommand)]
pub enum AgentCommands {
    /// Collect runtime diagnostics from one agent.
    Diagnostics { replica_id: Uuid },
    /// Stop one agent from accepting new actions while keeping management reachable.
    Drain { replica_id: Uuid },
    /// Restart one agent's broker worker loop.
    Restart { replica_id: Uuid },
    /// Fetch recent desktop-agent log lines.
    Logs {
        replica_id: Uuid,
        /// How many trailing log lines to ask for.
        #[arg(long, default_value_t = 200)]
        lines: usize,
    },
    /// List recent directive state for one agent.
    Directives {
        replica_id: Uuid,
        /// Cap on directives returned, newest first.
        #[arg(long, default_value_t = 50)]
        limit: i64,
    },
    /// Create a single-use enrollment token, shown only once.
    EnrollToken {
        /// Token lifetime, such as 30s, 15m, 2h, or 1d.
        #[arg(long, default_value = "15m")]
        ttl: String,
        /// Routing label as KEY=VALUE; repeat for multiple labels.
        #[arg(long = "label")]
        labels: Vec<String>,
        /// Organization assigned to the agent credential.
        #[arg(long)]
        org: Option<Uuid>,
        /// Service URL embedded in the token; defaults to --api-base-url.
        #[arg(long)]
        service_url: Option<String>,
        /// Cluster UUID bound to discovery; use when the LAN announcement URL differs.
        #[arg(long)]
        cluster_id: Option<Uuid>,
        /// Optional base64 SHA-256 SPKI pin for private/self-signed TLS.
        #[arg(long)]
        spki_pin: Option<String>,
    },
    /// List enrollment-token metadata. Secrets are never returned.
    EnrollmentTokens,
    /// Revoke an unused enrollment token.
    RevokeToken { token_id: String },
}

#[derive(Debug, Subcommand)]
pub enum OrgCommands {
    /// List the organizations you belong to, with your role in each.
    List,
    /// Create an organization; you become its owner.
    Create { name: String },
    /// Show an org's dedicated node allocation and projected monthly cost.
    Nodes { org: uuid::Uuid },
    /// Set an org's dedicated node count for a kind on a backend (quota-enforced).
    Scale {
        org: uuid::Uuid,
        /// Provisioning backend the nodes live on.
        #[arg(long, value_enum)]
        backend: CliProvisionBackend,
        /// Node kind to size.
        #[arg(long, value_enum)]
        kind: CliNodeKind,
        /// Exact number of nodes this org should hold.
        #[arg(long)]
        desired: u32,
    },
    /// Show an org's accrued usage and cost over the trailing 30 days.
    Usage { org: uuid::Uuid },
}

#[derive(Debug, Subcommand)]
pub enum NodeCommands {
    /// List provisioning backends and current node group sizing.
    List,
    /// Add nodes of a kind on a backend, raising the desired count by --count.
    SpinUp {
        /// Provisioning backend to add nodes on.
        #[arg(long, value_enum)]
        backend: CliProvisionBackend,
        /// Node kind to add.
        #[arg(long, value_enum)]
        kind: CliNodeKind,
        /// How many nodes to add to the current desired count.
        #[arg(long, default_value_t = 1)]
        count: u32,
        /// Routing label as KEY=VALUE applied to spun-up nodes; repeat for multiple.
        #[arg(long = "label")]
        labels: Vec<String>,
    },
    /// Set the exact desired node count for a kind on a backend.
    Scale {
        /// Provisioning backend the node group lives on.
        #[arg(long, value_enum)]
        backend: CliProvisionBackend,
        /// Node kind to size.
        #[arg(long, value_enum)]
        kind: CliNodeKind,
        /// Exact number of nodes to run.
        #[arg(long)]
        desired: u32,
    },
    /// Stop and remove a single node instance by id.
    Stop {
        /// Provisioning backend the node runs on.
        #[arg(long, value_enum)]
        backend: CliProvisionBackend,
        /// Instance id, as listed by `nodes list`.
        #[arg(long)]
        node: String,
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
        /// Which store the slot lives in.
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
        /// Which store the slot lives in.
        #[arg(long, value_enum, default_value_t = CliSettingKind::Secret)]
        kind: CliSettingKind,
        /// JSON-schema for a config value (json text), required on first write of a config slot.
        #[arg(long)]
        schema: Option<String>,
    },
    /// Import settings from an `.rrx` source containing a `settings` block. JSON is not accepted.
    Import { file: PathBuf },
    /// Delete a setting.
    Delete {
        scope: String,
        name: String,
        /// Which store the slot lives in.
        #[arg(long, value_enum, default_value_t = CliSettingKind::Secret)]
        kind: CliSettingKind,
    },
}

#[derive(Debug, Subcommand)]
pub enum RexRapCommands {
    /// Compile a workflow block from an .rrx source into a workflow definition JSON.
    Compile {
        file: PathBuf,
        /// Write the JSON here instead of to stdout.
        #[arg(short, long)]
        output: Option<PathBuf>,
        /// REXRAP type checking policy. Use permissive only for legacy investigation.
        #[arg(long, value_enum, default_value_t = CliTyping::Strict)]
        typing: CliTyping,
    },
    /// Decompile a workflow definition JSON file back into .rrx source.
    Decompile {
        file: PathBuf,
        /// Write the .rrx source here instead of to stdout.
        #[arg(short, long)]
        output: Option<PathBuf>,
        /// Emit the canonical fully-explicit form: start edge, ids and arrows on every node,
        /// and all defaulted values (timeout/retry/limit/concurrency/approval type).
        #[arg(long)]
        explicit: bool,
    },
    /// Format an .rrx source.
    Format {
        file: PathBuf,
        /// Write the formatted source here instead of over the file.
        #[arg(short, long)]
        output: Option<PathBuf>,
        /// Report whether the file is already formatted, changing nothing.
        #[arg(long)]
        check: bool,
    },
    /// Parse, lower, and validate an .rrx source, printing any diagnostics.
    Check {
        file: PathBuf,
        /// REXRAP type checking policy. Use permissive only for legacy investigation.
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
    /// Import a workflow pack (an .rrx source or directory of .rrx sources), or save a workflow
    /// definition / import a workflow bundle from a JSON file. A source's `settings` blocks are
    /// imported with the pack to seed config/secret slots. When no path is given, falls back to the
    /// `~/.runinator/workflows` folder if it exists.
    Apply { file: Option<PathBuf> },
    /// Dry-run a workflow pack against `tests` blocks in .rrx sources: simulate the state machine offline with
    /// mocked task outputs and assert on the branch taken and final outputs. No server required.
    Test {
        /// Workflow pack source (.rrx or a directory of .rrx sources).
        file: PathBuf,
        /// Additional .rrx sources containing `tests` blocks. When omitted, pack sources are used.
        #[arg(long = "tests", value_name = "PATH")]
        tests: Vec<PathBuf>,
        /// Only run cases whose name contains this substring.
        #[arg(long)]
        filter: Option<String>,
    },
    /// Watch a workflow pack, re-apply it on changes, and optionally run a workflow.
    Dev {
        file: Option<PathBuf>,
        /// Workflow id or name to run after each successful apply.
        #[arg(long)]
        run: Option<String>,
        /// Run parameter as KEY=VALUE; repeat for several. Values parse as json when they can.
        #[arg(long = "param", value_name = "KEY=VALUE")]
        params: Vec<String>,
        /// Read the run parameters from a json file instead.
        #[arg(long = "json-file")]
        json_file: Option<PathBuf>,
        /// Start the run paused for the debugger.
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
        /// Write the bundle here instead of to stdout.
        #[arg(short, long)]
        output: Option<PathBuf>,
    },
    /// List a workflow's revision history, newest first.
    Revisions {
        /// Workflow id or name.
        workflow: String,
        /// Cap on revisions listed, newest first.
        #[arg(long, default_value_t = 20)]
        limit: i64,
    },
    /// Show one revision, including the definition it captured.
    Revision {
        /// Workflow id or name.
        workflow: String,
        /// Revision number, as listed by `workflows revisions`.
        revision: i64,
    },
    /// Restore an earlier revision as the workflow's current definition. The restore is saved as a
    /// new revision, so nothing is overwritten and the rollback itself stays in the history.
    Rollback {
        /// Workflow id or name.
        workflow: String,
        /// Revision number to restore.
        revision: i64,
    },
    /// Duplicate a workflow into a new version sharing its name (default bump: minor).
    Duplicate {
        /// Workflow id or name to duplicate.
        workflow: String,
        /// How much of the version to raise on the copy.
        #[arg(long, value_enum, default_value_t = CliBumpLevel::default())]
        bump: CliBumpLevel,
    },
    /// Create a workflow run.
    Run {
        workflow: String,
        /// Run parameter as KEY=VALUE; repeat for several. Values parse as json when they can.
        #[arg(long = "param", value_name = "KEY=VALUE")]
        params: Vec<String>,
        /// Read the run parameters from a json file instead.
        #[arg(long = "json-file")]
        json_file: Option<PathBuf>,
        /// Start the run paused for the debugger.
        #[arg(long)]
        debug: bool,
        /// Name assigned to the created run.
        #[arg(long)]
        name: Option<String>,
    },
}

#[derive(Debug, Subcommand)]
pub enum RunCommands {
    /// List recent or filtered workflow runs.
    List {
        /// Only runs in this status, such as running, waiting, or failed.
        #[arg(long)]
        status: Option<String>,
        /// Only runs of one workflow.
        #[arg(long = "workflow-id")]
        workflow_id: Option<Uuid>,
        /// Only runs that have not reached a terminal status.
        #[arg(long)]
        open: bool,
    },
    /// Show a workflow run and its VM execution records.
    Show { id: Uuid },
    /// Refresh a workflow run until interrupted or terminal.
    Watch {
        id: Uuid,
        /// How often to refresh.
        #[arg(long, default_value_t = 2)]
        interval_seconds: u64,
    },
    /// Print log chunks for a workflow effect.
    Logs { effect_id: Uuid },
    /// Pause a workflow run.
    Pause { id: Uuid },
    /// Resume a workflow run.
    Resume { id: Uuid },
    /// Cancel a workflow run.
    Cancel { id: Uuid },
    /// Permanently delete a workflow run and its execution history.
    Delete { id: Uuid },
    /// Replay a workflow run.
    Replay {
        id: Uuid,
        /// Replay from this node id rather than from the start.
        #[arg(long = "from-step")]
        from_step_id: Option<String>,
    },
    /// Rename a workflow run.
    Rename { id: Uuid, name: Option<String> },
    /// List the run-level artifacts a workflow run produced.
    Artifacts { id: Uuid },
}

#[derive(Debug, Subcommand)]
pub enum ArtifactCommands {
    /// List the artifacts a workflow effect produced.
    List {
        /// The effect whose artifacts to list.
        #[arg(long = "effect")]
        effect_id: Uuid,
    },
}

#[derive(Debug, Subcommand)]
pub enum ApprovalCommands {
    /// List approval requests.
    List {
        /// Only requests raised by one run.
        #[arg(long = "workflow-run-id")]
        workflow_run_id: Option<Uuid>,
        /// Only requests still awaiting a decision.
        #[arg(long)]
        open: bool,
    },
    /// Approve an approval request.
    Approve {
        /// Durable approval effect id.
        id: Uuid,
        /// Note recorded with the decision.
        #[arg(long)]
        message: Option<String>,
        /// Json payload handed back to the workflow.
        #[arg(long = "json-file")]
        json_file: Option<PathBuf>,
    },
    /// Reject an approval request.
    Reject {
        /// Durable approval effect id.
        id: Uuid,
        /// Note recorded with the decision.
        #[arg(long)]
        message: Option<String>,
        /// Json payload handed back to the workflow.
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
        /// Run parameter as KEY=VALUE; repeat for several.
        #[arg(long = "param", value_name = "KEY=VALUE")]
        params: Vec<String>,
        /// Read the run parameters from a json file instead.
        #[arg(long = "json-file")]
        json_file: Option<PathBuf>,
        /// Start the run paused for the debugger.
        #[arg(long)]
        debug: bool,
    },
    /// Replay a cron trigger's slots across a past range. Slots already fired keep their original
    /// run, so an overlapping range is safe to re-issue.
    Backfill {
        trigger_id: Uuid,
        /// Start of the range, exclusive (RFC 3339, e.g. 2026-08-01T00:00:00Z).
        #[arg(long)]
        from: DateTime<Utc>,
        /// End of the range, inclusive (RFC 3339). Defaults to now.
        #[arg(long)]
        to: Option<DateTime<Utc>>,
        /// Cap on slots replayed.
        #[arg(long)]
        limit: Option<i64>,
        /// Report the slots that would fire without creating any runs.
        #[arg(long)]
        dry_run: bool,
    },
}

#[derive(Debug, Subcommand)]
pub enum FreezeCommands {
    /// List freeze windows.
    List {
        /// Show only the windows in effect right now.
        #[arg(long)]
        active: bool,
    },
    /// Suspend trigger firing over a time range.
    Create {
        name: String,
        /// Start of the window (RFC 3339).
        #[arg(long)]
        from: DateTime<Utc>,
        /// End of the window (RFC 3339).
        #[arg(long)]
        to: DateTime<Utc>,
        /// Freeze one workflow rather than everything in scope.
        #[arg(long)]
        workflow_id: Option<Uuid>,
        /// Freeze one org rather than the whole platform.
        #[arg(long)]
        org_id: Option<Uuid>,
        /// Why the window exists, shown wherever it is listed.
        #[arg(long)]
        reason: Option<String>,
    },
    /// Remove a freeze window.
    Delete { window_id: Uuid },
}

#[derive(Debug, Subcommand)]
pub enum PipelineCommands {
    /// List pipelines.
    List,
    /// Show a pipeline by id or name, with its member workflows.
    Show { pipeline: String },
    /// Start a pipeline run.
    Run {
        /// Pipeline id or name.
        pipeline: String,
        /// Run parameter as KEY=VALUE; repeat for several. Values parse as json when they can.
        #[arg(long = "param", value_name = "KEY=VALUE")]
        params: Vec<String>,
        /// Read the run parameters from a json file instead.
        #[arg(long = "json-file")]
        json_file: Option<PathBuf>,
        /// Run this immutable pipeline revision instead of the current head.
        #[arg(long)]
        revision: Option<i64>,
        /// Wait for the run to reach a terminal status before returning.
        #[arg(long)]
        follow: bool,
    },
    /// List a pipeline's immutable revision history, newest first.
    Revisions {
        pipeline: String,
        #[arg(long, default_value_t = 20)]
        limit: i64,
    },
    /// Show one immutable pipeline revision and its digest.
    Revision { pipeline: String, revision: i64 },
    /// List pipeline runs, newest first.
    Runs {
        /// Only runs of one pipeline, by id or name.
        #[arg(long)]
        pipeline: Option<String>,
    },
    /// Show a pipeline run and the member workflow runs it started.
    RunShow { run_id: Uuid },
    /// Cancel a pipeline run.
    Cancel { run_id: Uuid },
    /// Permanently delete a pipeline run and its member workflow history.
    DeleteRun { run_id: Uuid },
    /// Pause a pipeline run.
    Pause { run_id: Uuid },
    /// Resume a paused pipeline run.
    Resume { run_id: Uuid },
    /// Resolve a pipeline run paused on an `inquire` member failure.
    Resolve {
        run_id: Uuid,
        /// Continue the pipeline or abort it.
        #[arg(long, value_enum, default_value_t = CliInquiryDecision::Continue)]
        decision: CliInquiryDecision,
        /// Who is deciding, recorded on the run.
        #[arg(long)]
        by: Option<String>,
        /// Note recorded with the decision.
        #[arg(long)]
        message: Option<String>,
    },
    /// Retry a failed or timed-out member that is still on the pipeline frontier.
    Retry {
        run_id: Uuid,
        member: String,
        #[arg(long = "param", value_name = "KEY=VALUE")]
        params: Vec<String>,
        #[arg(long = "json-file")]
        json_file: Option<PathBuf>,
    },
    /// Delete a pipeline. Its member workflows are untouched.
    Delete { pipeline: String },
}

/// CLI-facing decision for a pipeline run's open `inquire` pause.
#[derive(Debug, Clone, Copy, Default, ValueEnum)]
pub enum CliInquiryDecision {
    #[default]
    Continue,
    Abort,
}

impl CliInquiryDecision {
    pub fn as_str(self) -> &'static str {
        match self {
            CliInquiryDecision::Continue => "continue",
            CliInquiryDecision::Abort => "abort",
        }
    }
}

#[derive(Debug, Subcommand)]
pub enum FunctionCommands {
    /// Check a package directory and print the digest a publish would upload. Runs offline.
    Validate {
        /// Directory holding a `runinator-function.json` manifest.
        path: PathBuf,
    },
    /// Publish one version of a package.
    Publish {
        /// Directory holding a `runinator-function.json` manifest.
        path: PathBuf,
        /// Alias to move onto the new version, overriding the manifest's.
        #[arg(long)]
        alias: Option<String>,
    },
    /// Call a published export and print what it returned.
    Invoke {
        /// Export to call, as `package.export`.
        target: String,
        /// Resolve the version this alias names.
        #[arg(long)]
        alias: Option<String>,
        /// Call this exact version instead of the alias.
        #[arg(long)]
        version: Option<i64>,
        /// Input payload as inline json.
        #[arg(long)]
        input: Option<String>,
        /// Read the input payload from a json file instead.
        #[arg(long = "input-file")]
        input_file: Option<PathBuf>,
    },
    /// List published packages.
    List,
    /// Show one package with its versions, aliases, and exports.
    Show { package: String },
    /// List every published export as a catalog entry.
    Catalog,
    /// List a package's versions.
    Versions { package: String },
    /// Point an alias at a version.
    Alias {
        package: String,
        alias: String,
        /// Target version number. Omitted with no `--from`, the newest version is used.
        #[arg(long)]
        version: Option<i64>,
        /// Target the version another alias currently names.
        #[arg(long)]
        from: Option<String>,
    },
    /// Delete an alias. The version it named is untouched.
    Unalias { package: String, alias: String },
    /// Archive a package while retaining versions pinned by workflows.
    Delete { package: String },
    /// Restore an archived package.
    Restore { package: String },
}

#[derive(Debug, Subcommand)]
pub enum ProviderCommands {
    /// List providers.
    List,
    /// Show one provider by name.
    Show { name: String },
}
