use std::path::PathBuf;

use clap::{Parser, Subcommand};

#[derive(Debug, Parser)]
#[command(
    name = "runinator-supervisor",
    about = "Lightweight local process supervisor for Runinator services"
)]
pub struct Cli {
    #[arg(
        short,
        long,
        global = true,
        default_value = "runinator-supervisor.json"
    )]
    pub config: PathBuf,

    #[command(subcommand)]
    pub command: Commands,
}

#[derive(Debug, Subcommand)]
pub enum Commands {
    /// Start the supervisor daemon.
    Start {
        /// Run in the foreground instead of daemon mode.
        #[arg(long, default_value_t = false)]
        foreground: bool,
    },
    /// Stop the supervisor and all managed child processes.
    Stop,
    /// Stop and start the supervisor daemon.
    Restart {
        /// Run in the foreground instead of daemon mode after stopping.
        #[arg(long, default_value_t = false)]
        foreground: bool,
    },
    /// Show a table of managed process state.
    Status {
        /// Refresh continuously.
        #[arg(long, default_value_t = false)]
        watch: bool,
    },
    /// Show tails of managed process logs.
    Logs {
        /// Exact process name to show. Defaults to all processes.
        #[arg(short, long)]
        process: Option<String>,
        /// Number of lines to show per process.
        #[arg(short, long, default_value_t = 80)]
        lines: usize,
        /// Refresh continuously.
        #[arg(short, long, default_value_t = false)]
        watch: bool,
    },
    /// Add, start, stop, or remove a dynamic process in the running supervisor.
    Process {
        #[command(subcommand)]
        command: ProcessCommands,
    },
    #[command(hide = true)]
    Supervise {
        #[arg(long, default_value_t = false)]
        foreground: bool,
    },
}

#[derive(Debug, Subcommand)]
pub enum ProcessCommands {
    /// Register (and start, unless --no-autostart) a new process.
    Add {
        #[arg(long)]
        name: String,
        #[arg(long)]
        command: String,
        /// A command-line argument; repeat for multiple.
        #[arg(long = "arg")]
        args: Vec<String>,
        /// An environment variable as KEY=VALUE; repeat for multiple.
        #[arg(long = "env")]
        env: Vec<String>,
        #[arg(long)]
        cwd: Option<String>,
        /// Do not start the process immediately after adding it.
        #[arg(long, default_value_t = false)]
        no_autostart: bool,
    },
    /// Start a registered process that is stopped.
    Start { name: String },
    /// Stop a running process without removing it.
    Stop { name: String },
    /// Stop and forget a process.
    Remove { name: String },
}
