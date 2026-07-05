mod cli;
mod config;
mod control;
mod display;
mod os;
mod snapshot;
mod supervisor;
mod types;

use std::collections::BTreeMap;

use clap::Parser;

use crate::{
    cli::{Cli, Commands, ProcessCommands},
    config::ProcessConfig,
    control::{ControlCommand, enqueue},
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
        Commands::Process { command } => run_process_command(command, &paths)?,
        Commands::Supervise { foreground } => run_supervisor(&config, &paths, foreground)?,
    }

    Ok(())
}

fn run_process_command(command: ProcessCommands, paths: &config::Paths) -> Result<(), DynError> {
    let control_command = match command {
        ProcessCommands::Add {
            name,
            command,
            args,
            env,
            cwd,
            no_autostart,
        } => {
            let mut env_map = BTreeMap::new();
            for entry in env {
                let (key, value) = entry
                    .split_once('=')
                    .ok_or_else(|| format!("Invalid --env '{entry}', expected KEY=VALUE"))?;
                env_map.insert(key.to_string(), value.to_string());
            }
            ControlCommand::AddProcess {
                process: ProcessConfig {
                    name,
                    command,
                    args,
                    cwd,
                    env: env_map,
                    autostart: !no_autostart,
                    restart_on_failure: true,
                    max_restarts_per_minute: 10,
                    command_windows: None,
                    args_windows: None,
                },
            }
        }
        ProcessCommands::Start { name } => ControlCommand::StartProcess { name },
        ProcessCommands::Stop { name } => ControlCommand::StopProcess { name },
        ProcessCommands::Remove { name } => ControlCommand::RemoveProcess { name },
    };

    enqueue(&paths.control_dir, &control_command)?;
    println!(
        "Queued control command for supervisor at {}.",
        paths.control_dir.display()
    );
    Ok(())
}
