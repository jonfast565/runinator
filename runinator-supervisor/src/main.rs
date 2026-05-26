mod cli;
mod config;
mod display;
mod os;
mod snapshot;
mod supervisor;
mod types;

use clap::Parser;

use crate::{
    cli::{Cli, Commands},
    display::{show_logs, show_status},
    supervisor::{run_supervisor, start_daemon, stop_supervisor},
    types::DynError,
};

fn main() -> Result<(), DynError> {
    let cli = Cli::parse();
    let (config, paths) = config::load_config(&cli.config)?;

    match cli.command {
        Commands::Start { foreground } => {
            if foreground {
                run_supervisor(&config, &paths, true)?;
            } else {
                start_daemon(&paths)?;
            }
        }
        Commands::Stop => stop_supervisor(&config, &paths)?,
        Commands::Restart { foreground } => {
            stop_supervisor(&config, &paths)?;
            if foreground {
                run_supervisor(&config, &paths, true)?;
            } else {
                start_daemon(&paths)?;
            }
        }
        Commands::Status { watch } => show_status(&paths, watch)?,
        Commands::Logs {
            process,
            lines,
            watch,
        } => show_logs(&paths, process.as_deref(), lines, watch)?,
        Commands::Supervise { foreground } => run_supervisor(&config, &paths, foreground)?,
    }

    Ok(())
}
