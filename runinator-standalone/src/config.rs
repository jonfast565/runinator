use std::path::PathBuf;

use clap::{Parser, Subcommand};
use serde::{Deserialize, Serialize};

#[derive(Debug, Subcommand)]
pub enum Command {
    Start {
        /// run attached to the current terminal.
        #[arg(long)]
        foreground: bool,
        /// show the interactive runtime dashboard; implies `--foreground`.
        #[arg(long, env = "RUNINATOR_TUI", default_value_t = false)]
        tui: bool,
    },
    Stop,
    Restart {
        /// run attached to the current terminal after restarting.
        #[arg(long)]
        foreground: bool,
        /// show the interactive runtime dashboard; implies `--foreground`.
        #[arg(long, env = "RUNINATOR_TUI", default_value_t = false)]
        tui: bool,
    },
    Status {
        #[arg(long)]
        watch: bool,
    },
    Logs {
        #[arg(long, alias = "process")]
        component: Option<String>,
        #[arg(long, default_value_t = 80)]
        lines: usize,
        #[arg(long)]
        watch: bool,
    },
    /// open the operations console without starting or stopping a runtime.
    Tui,
    #[command(hide = true)]
    Serve,
}

impl Command {
    pub fn runs_foreground(&self) -> bool {
        match self {
            Self::Start { foreground, tui } | Self::Restart { foreground, tui } => {
                *foreground || *tui
            }
            Self::Serve => true,
            Self::Stop | Self::Status { .. } | Self::Logs { .. } | Self::Tui => false,
        }
    }

    pub fn tui_requested(&self) -> bool {
        matches!(
            self,
            Self::Start { tui: true, .. } | Self::Restart { tui: true, .. }
        )
    }
}

pub(crate) fn absolute_from(base: &std::path::Path, path: PathBuf) -> PathBuf {
    if path.is_absolute() {
        path
    } else {
        base.join(path)
    }
}

pub(crate) fn validate_wsl_path(
    path: &std::path::Path,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    #[cfg(not(windows))]
    {
        let value = path.to_string_lossy();
        if value.as_bytes().get(1) == Some(&b':')
            && value
                .as_bytes()
                .first()
                .is_some_and(u8::is_ascii_alphabetic)
        {
            return Err(format!(
                "Windows path '{value}' cannot be opened by a Linux/WSL process; use its /mnt/<drive>/... path"
            )
            .into());
        }
    }
    Ok(())
}

#[cfg(test)]
#[path = "config_tests.rs"]
mod tests;

mod cli;
pub use cli::Cli;

mod standalone_config;
pub use standalone_config::StandaloneConfig;
