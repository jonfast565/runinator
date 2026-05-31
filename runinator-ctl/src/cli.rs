use std::path::PathBuf;

use clap::{Parser, Subcommand};

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

    #[arg(long, global = true)]
    pub json: bool,

    #[command(subcommand)]
    pub command: Commands,
}

#[derive(Debug, Subcommand)]
pub enum Commands {
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
    /// Compile, decompile, format, and check the wdl workflow language.
    Wdl {
        #[command(subcommand)]
        command: WdlCommands,
    },
}

#[derive(Debug, Subcommand)]
pub enum WdlCommands {
    /// Compile a .wdl file into a workflow definition JSON.
    Compile {
        file: PathBuf,
        #[arg(short, long)]
        output: Option<PathBuf>,
    },
    /// Decompile a workflow definition JSON file back into .wdl source.
    Decompile {
        file: PathBuf,
        #[arg(short, long)]
        output: Option<PathBuf>,
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
    Check { file: PathBuf },
}

#[derive(Debug, Subcommand)]
pub enum WorkflowCommands {
    /// List workflow definitions.
    List,
    /// Show a workflow by id or name.
    Show { workflow: String },
    /// Validate a workflow definition JSON file.
    Validate { file: PathBuf },
    /// Save a workflow definition or import a workflow bundle JSON file.
    Apply { file: PathBuf },
    /// Export one workflow or the full workflow bundle.
    Export {
        workflow_id: Option<i64>,
        #[arg(short, long)]
        output: Option<PathBuf>,
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
        workflow_id: Option<i64>,
        #[arg(long)]
        open: bool,
    },
    /// Show a workflow run and its node runs.
    Show { id: i64 },
    /// Refresh a workflow run until interrupted or terminal.
    Watch {
        id: i64,
        #[arg(long, default_value_t = 2)]
        interval_seconds: u64,
    },
    /// Print log chunks for a workflow node run.
    Logs {
        node_run_id: i64,
        #[arg(long)]
        cursor: Option<i64>,
        #[arg(long, default_value_t = 100)]
        limit: i64,
    },
    /// Pause a workflow run.
    Pause { id: i64 },
    /// Resume a workflow run.
    Resume { id: i64 },
    /// Cancel a workflow run.
    Cancel { id: i64 },
    /// Replay a workflow run.
    Replay {
        id: i64,
        #[arg(long = "from-step")]
        from_step_id: Option<String>,
    },
    /// Rename a workflow run.
    Rename { id: i64, name: Option<String> },
}

#[derive(Debug, Subcommand)]
pub enum ApprovalCommands {
    /// List approval requests.
    List {
        #[arg(long = "workflow-run-id")]
        workflow_run_id: Option<i64>,
        #[arg(long)]
        open: bool,
    },
    /// Approve an approval request.
    Approve {
        id: i64,
        #[arg(long)]
        by: Option<String>,
        #[arg(long)]
        message: Option<String>,
        #[arg(long = "json-file")]
        json_file: Option<PathBuf>,
    },
    /// Reject an approval request.
    Reject {
        id: i64,
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
        trigger_id: i64,
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
