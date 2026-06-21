use log::{error, info};
use runinator_models::errors::SendableError;
use std::{env, fs, fs::File, path::PathBuf, sync::Mutex};
use tracing_subscriber::{EnvFilter, fmt, prelude::*};

use crate::app_data;

/// install the global tracing subscriber: structured spans/events to stdout plus a log file, with
/// the existing `log` macros bridged in. honors `RUNINATOR_LOG` (an `EnvFilter` directive); falls
/// back to `info`.
pub fn setup_logger() -> Result<(), SendableError> {
    let log_file = open_log_file()?;

    let filter = EnvFilter::try_from_env("RUNINATOR_LOG")
        .or_else(|_| EnvFilter::try_new("info"))
        .map_err(|err| -> SendableError { Box::new(err) })?;

    let stdout_layer = fmt::layer().with_target(true).with_writer(std::io::stdout);
    let file_layer = fmt::layer()
        .with_ansi(false)
        .with_target(true)
        .with_writer(Mutex::new(log_file));

    tracing_subscriber::registry()
        .with(filter)
        .with(stdout_layer)
        .with(file_layer)
        .try_init()
        .map_err(|err| -> SendableError { Box::new(std::io::Error::other(err.to_string())) })?;

    Ok(())
}

pub fn print_env() -> std::io::Result<()> {
    let path = env::current_dir()?;
    info!("The current directory is {}", path.display());
    Ok(())
}

fn open_log_file() -> std::io::Result<File> {
    let mut last_error: Option<std::io::Error> = None;
    let mut had_failure = false;

    for path in desired_log_paths() {
        let path_string = path.display().to_string();
        if let Some(parent) = path.parent()
            && let Err(err) = fs::create_dir_all(parent)
        {
            had_failure = true;
            error!(
                "Failed to create log directory at {}: {}",
                parent.display(),
                err
            );
            last_error = Some(err);
            continue;
        }
        match fs::OpenOptions::new().create(true).append(true).open(&path) {
            Ok(file) => {
                if had_failure {
                    error!("Falling back to log file at {}", path_string);
                }
                return Ok(file);
            }
            Err(err) => {
                had_failure = true;
                error!("Failed to open log file at {}: {}", path_string, err);
                last_error = Some(err);
            }
        }
    }

    Err(last_error
        .unwrap_or_else(|| std::io::Error::other("unable to open log file at any known location")))
}

fn desired_log_paths() -> Vec<PathBuf> {
    let mut paths = Vec::new();

    if let Ok(custom) = env::var("RUNINATOR_LOG_PATH")
        && !custom.trim().is_empty()
    {
        paths.push(PathBuf::from(custom));
    }

    match app_data::default_log_path() {
        Ok(path) => paths.push(path),
        Err(err) => error!("Failed to resolve Runinator log path: {}", err),
    }

    paths.push(env::temp_dir().join("runinator-output.log"));

    paths
}
