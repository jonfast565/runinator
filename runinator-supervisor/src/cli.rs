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
    #[command(hide = true)]
    Supervise {
        #[arg(long, default_value_t = false)]
        foreground: bool,
    },
}
