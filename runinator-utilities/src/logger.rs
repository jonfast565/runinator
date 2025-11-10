use log::{error, info};
use runinator_models::errors::SendableError;
use std::{env, path::PathBuf, time::SystemTime};

pub fn setup_logger() -> Result<(), SendableError> {
    let log_output = open_log_output()?;

    fern::Dispatch::new()
        .format(|out, message, record| {
            out.finish(format_args!(
                "[{} {} {}] {}",
                humantime::format_rfc3339_seconds(SystemTime::now()),
                record.level(),
                record.target(),
                message
            ))
        })
        .level(log::LevelFilter::Info)
        .chain(std::io::stdout())
        .chain(log_output)
        .apply()?;
    Ok(())
}

pub fn print_env() -> std::io::Result<()> {
    let path = env::current_dir()?;
    info!("The current directory is {}", path.display());
    Ok(())
}

fn open_log_output() -> std::io::Result<fern::Output> {
    let mut last_error: Option<std::io::Error> = None;
    let mut had_failure = false;

    for path in desired_log_paths() {
        let path_string = path.display().to_string();
        match fern::log_file(&path) {
            Ok(output) => {
                if had_failure {
                    error!(
                        "Falling back to log file at {}",
                        path_string
                    );
                }
                return Ok(fern::Output::from(output));
            }
            Err(err) => {
                had_failure = true;
                error!(
                    "Failed to open log file at {}: {}",
                    path_string, err
                );
                last_error = Some(err);
            }
        }
    }

    Err(last_error.unwrap_or_else(|| {
        std::io::Error::new(
            std::io::ErrorKind::Other,
            "unable to open log file at any known location",
        )
    }))
}

fn desired_log_paths() -> Vec<PathBuf> {
    let mut paths = Vec::new();

    if let Ok(custom) = env::var("RUNINATOR_LOG_PATH") {
        if !custom.trim().is_empty() {
            paths.push(PathBuf::from(custom));
        }
    }

    // Original behaviour: attempt to log alongside the binary/working dir.
    paths.push(PathBuf::from("output.log"));

    // Portable fallback that should always be writable inside containers.
    paths.push(env::temp_dir().join("runinator-output.log"));

    paths
}
