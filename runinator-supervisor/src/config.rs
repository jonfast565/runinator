use std::{
    collections::BTreeMap,
    env, fs, io,
    path::{Path, PathBuf},
};

use serde::{Deserialize, Serialize};

use crate::types::DynError;

pub fn load_config(path: &Path) -> Result<(SupervisorConfig, Paths), DynError> {
    let cwd = env::current_dir()?;
    let config_path = if path.is_absolute() {
        path.to_path_buf()
    } else {
        cwd.join(path)
    };
    let config_path = config_path.canonicalize().map_err(|err| {
        io::Error::new(
            io::ErrorKind::NotFound,
            format!(
                "Unable to resolve config path {}: {err}",
                config_path.display()
            ),
        )
    })?;
    let config_dir = config_path
        .parent()
        .ok_or_else(|| io::Error::other("Config path has no parent directory"))?
        .to_path_buf();

    let data = fs::read_to_string(&config_path)?;
    let config: SupervisorConfig = serde_json::from_str(&data).map_err(|err| {
        io::Error::new(
            io::ErrorKind::InvalidData,
            format!("Invalid config JSON in {}: {err}", config_path.display()),
        )
    })?;

    if config.processes.is_empty() {
        return Err(io::Error::new(io::ErrorKind::InvalidInput, "Config has no processes").into());
    }

    let state_dir = resolve_path(&config_dir, Path::new(&config.state_dir));
    let paths = Paths {
        config_path,
        config_dir,
        pid_file: state_dir.join("supervisor.pid"),
        stop_file: state_dir.join("stop"),
        state_file: state_dir.join("state.json"),
        control_dir: state_dir.join("control"),
        logs_dir: state_dir.join("logs"),
        supervisor_log: state_dir.join("supervisor.log"),
        state_dir,
    };

    Ok((config, paths))
}

pub fn resolve_path(base_dir: &Path, raw: &Path) -> PathBuf {
    if raw.is_absolute() {
        raw.to_path_buf()
    } else {
        base_dir.join(raw)
    }
}

fn default_true() -> bool {
    true
}

fn default_state_dir() -> String {
    runinator_platform::app_data::default_supervisor_state_dir()
        .map(|path| path.to_string_lossy().into_owned())
        .unwrap_or_else(|_| ".runinator/supervisor".to_string())
}

fn default_shutdown_timeout_secs() -> u64 {
    10
}

fn default_restart_delay_ms() -> u64 {
    2000
}

fn default_log_retention_max_age_days() -> u64 {
    7
}

fn default_log_retention_max_files() -> usize {
    200
}

fn default_log_retention_max_bytes() -> u64 {
    512 * 1024 * 1024
}

fn default_max_restarts_per_minute() -> u32 {
    10
}

mod supervisor_config;
pub use supervisor_config::SupervisorConfig;

mod log_retention_config;
pub use log_retention_config::LogRetentionConfig;

mod process_config;
pub use process_config::ProcessConfig;

mod paths;
pub use paths::Paths;
