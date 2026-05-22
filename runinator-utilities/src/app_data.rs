use std::{
    env, io,
    path::{Path, PathBuf},
};

use runinator_models::errors::SendableError;

pub const APP_DATA_DIR_NAME: &str = ".runinator";

pub fn app_data_dir() -> Result<PathBuf, SendableError> {
    if let Some(path) = env::var_os("RUNINATOR_HOME").filter(|path| !path.is_empty()) {
        return Ok(PathBuf::from(path));
    }

    let Some(home) = env::var_os("HOME")
        .filter(|path| !path.is_empty())
        .or_else(|| env::var_os("USERPROFILE").filter(|path| !path.is_empty()))
    else {
        return Err(Box::new(io::Error::new(
            io::ErrorKind::NotFound,
            "unable to resolve home directory for Runinator app data",
        )));
    };

    Ok(PathBuf::from(home).join(APP_DATA_DIR_NAME))
}

pub fn app_data_path(path: impl AsRef<Path>) -> Result<PathBuf, SendableError> {
    Ok(app_data_dir()?.join(path))
}

pub fn default_log_path() -> Result<PathBuf, SendableError> {
    app_data_path("logs/output.log")
}

pub fn default_sqlite_path() -> Result<PathBuf, SendableError> {
    app_data_path("runinator.db")
}

pub fn default_supervisor_state_dir() -> Result<PathBuf, SendableError> {
    app_data_path("supervisor")
}

pub fn default_supervisor_config_path() -> Result<PathBuf, SendableError> {
    app_data_path("runinator-supervisor.json")
}

pub fn default_secret_bundle_path() -> Result<PathBuf, SendableError> {
    app_data_path("secrets.json")
}
