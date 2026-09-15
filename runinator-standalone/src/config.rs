use std::path::PathBuf;

use clap::{Parser, Subcommand};
use serde::{Deserialize, Serialize};

#[derive(Debug, Parser)]
#[command(
    author,
    version,
    about = "Run the complete local Runinator stack in one process"
)]
pub struct Cli {
    #[command(subcommand)]
    pub command: Command,

    /// remote server used by the attached operations console.
    #[arg(long, env = "RUNINATOR_API_BASE_URL", global = true)]
    pub api_base_url: Option<String>,
    /// API key or access token used by the attached operations console.
    #[arg(long, env = "RUNINATOR_API_KEY", global = true)]
    pub api_key: Option<String>,

    #[arg(long, env = "RUNINATOR_STANDALONE_STATE_DIR", global = true)]
    pub state_dir: Option<PathBuf>,
    #[arg(
        long,
        env = "RUNINATOR_DATABASE",
        default_value = "sqlite",
        global = true
    )]
    pub database: String,
    #[arg(long, env = "RUNINATOR_SQLITE_PATH", global = true)]
    pub sqlite_path: Option<PathBuf>,
    #[arg(long, env = "RUNINATOR_DATABASE_URL", global = true)]
    pub database_url: Option<String>,
    #[arg(
        long,
        env = "RUNINATOR_STANDALONE_WORKERS",
        default_value_t = 1,
        global = true
    )]
    pub workers: u32,
    #[arg(
        long,
        env = "RUNINATOR_STANDALONE_ENGINES",
        default_value_t = 1,
        global = true
    )]
    pub engines: u32,
    #[arg(
        long,
        env = "RUNINATOR_STANDALONE_WAKERS",
        default_value_t = 1,
        global = true
    )]
    pub wakers: u32,
    #[arg(
        long,
        env = "RUNINATOR_MAX_CONCURRENT_ACTIONS",
        default_value_t = 4,
        global = true
    )]
    pub worker_concurrency: usize,
    #[arg(
        long,
        env = "RUNINATOR_MAX_CONCURRENT_INGRESS",
        default_value_t = 16,
        global = true
    )]
    pub engine_concurrency: usize,
    #[arg(long, env = "RUNINATOR_PORT", default_value_t = 8080, global = true)]
    pub api_port: u16,
    #[arg(
        long,
        env = "RUNINATOR_BROKER_PORT",
        default_value_t = 7070,
        global = true
    )]
    pub broker_port: u16,
    #[arg(
        long,
        env = "RUNINATOR_BLOB_PORT",
        default_value_t = 9100,
        global = true
    )]
    pub blob_port: u16,
    #[arg(
        long,
        env = "RUNINATOR_ADAPTER_HOST_PORT",
        default_value_t = 8790,
        global = true
    )]
    pub adapter_port: u16,
    #[arg(
        long,
        env = "RUNINATOR_AUTH_ENABLED",
        default_value_t = false,
        global = true
    )]
    pub auth_enabled: bool,
    #[arg(
        long,
        env = "RUNINATOR_STANDALONE_NO_DESKTOP_AGENT",
        default_value_t = false,
        global = true
    )]
    pub no_desktop_agent: bool,
    #[arg(long = "pack", env = "RUNINATOR_STANDALONE_PACK", global = true)]
    pub packs: Vec<PathBuf>,
    /// path to the runinatorctl executable used by startup pack imports.
    #[arg(long, env = "RUNINATOR_STANDALONE_CTL_PATH", global = true)]
    pub ctl_path: Option<PathBuf>,
    #[arg(
        long,
        env = "RUNINATOR_STANDALONE_NO_DEFAULT_PACK",
        default_value_t = false,
        global = true
    )]
    pub no_default_pack: bool,
    #[arg(long, hide = true, global = true)]
    pub config_json: Option<PathBuf>,
}

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

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StandaloneConfig {
    pub state_dir: PathBuf,
    pub database: String,
    pub sqlite_path: PathBuf,
    pub database_url: Option<String>,
    pub workers: u32,
    pub engines: u32,
    pub wakers: u32,
    pub worker_concurrency: usize,
    pub engine_concurrency: usize,
    pub api_port: u16,
    pub broker_port: u16,
    pub blob_port: u16,
    pub adapter_port: u16,
    pub auth_enabled: bool,
    pub desktop_agent: bool,
    pub packs: Vec<PathBuf>,
    #[serde(default)]
    pub ctl_path: Option<PathBuf>,
}

impl Cli {
    pub fn resolved_state_dir(&self) -> Result<PathBuf, Box<dyn std::error::Error + Send + Sync>> {
        let invoking_dir = std::env::current_dir()?;
        let state_path = self
            .state_dir
            .clone()
            .unwrap_or(runinator_platform::app_data::app_data_path("standalone")?);
        validate_wsl_path(&state_path)?;
        Ok(absolute_from(&invoking_dir, state_path))
    }

    pub fn resolved_config(
        &self,
    ) -> Result<StandaloneConfig, Box<dyn std::error::Error + Send + Sync>> {
        if let Some(path) = &self.config_json {
            let invoking_dir = std::env::current_dir()?;
            let config_path = absolute_from(&invoking_dir, path.clone());
            let mut config: StandaloneConfig =
                serde_json::from_slice(&std::fs::read(&config_path)?)?;
            let base = config_path.parent().unwrap_or(&invoking_dir);
            config.resolve_paths(base)?;
            return Ok(config);
        }
        let invoking_dir = std::env::current_dir()?;
        let state_dir = self.resolved_state_dir()?;
        let sqlite_source = self
            .sqlite_path
            .clone()
            .unwrap_or(runinator_platform::app_data::default_sqlite_path()?);
        validate_wsl_path(&sqlite_source)?;
        let sqlite_path = absolute_from(&invoking_dir, sqlite_source);
        let mut packs = self
            .packs
            .iter()
            .map(|path| {
                validate_wsl_path(path).map(|()| absolute_from(&invoking_dir, path.clone()))
            })
            .collect::<Result<Vec<_>, _>>()?;
        if packs.is_empty() && !self.no_default_pack {
            let default = invoking_dir.join("packs/hello-world");
            if default.exists() {
                packs.push(default);
            } else if matches!(
                &self.command,
                Command::Start { .. } | Command::Restart { .. } | Command::Serve
            ) {
                eprintln!(
                    "optional default pack was not found at {}; continuing without it",
                    default.display()
                );
            }
        }
        for pack in &packs {
            if !pack.exists() {
                return Err(
                    format!("standalone pack path does not exist: {}", pack.display()).into(),
                );
            }
        }
        let ctl_path = self
            .ctl_path
            .as_ref()
            .map(|path| {
                validate_wsl_path(path)?;
                Ok::<_, Box<dyn std::error::Error + Send + Sync>>(absolute_from(
                    &invoking_dir,
                    path.clone(),
                ))
            })
            .transpose()?;
        Ok(StandaloneConfig {
            state_dir,
            database: self.database.clone(),
            sqlite_path,
            database_url: self.database_url.clone(),
            workers: self.workers,
            engines: self.engines,
            wakers: self.wakers,
            worker_concurrency: self.worker_concurrency.max(1),
            engine_concurrency: self.engine_concurrency.max(1),
            api_port: self.api_port,
            broker_port: self.broker_port,
            blob_port: self.blob_port,
            adapter_port: self.adapter_port,
            auth_enabled: self.auth_enabled,
            desktop_agent: !self.no_desktop_agent,
            packs,
            ctl_path,
        })
    }
}

impl StandaloneConfig {
    fn resolve_paths(
        &mut self,
        base: &std::path::Path,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        validate_wsl_path(&self.state_dir)?;
        validate_wsl_path(&self.sqlite_path)?;
        self.state_dir = absolute_from(base, self.state_dir.clone());
        self.sqlite_path = absolute_from(base, self.sqlite_path.clone());
        for pack in &mut self.packs {
            validate_wsl_path(pack)?;
            *pack = absolute_from(base, pack.clone());
            if !pack.exists() {
                return Err(
                    format!("standalone pack path does not exist: {}", pack.display()).into(),
                );
            }
        }
        if let Some(path) = &mut self.ctl_path {
            validate_wsl_path(path)?;
            *path = absolute_from(base, path.clone());
        }
        Ok(())
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
