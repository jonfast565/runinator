use std::{path::PathBuf, time::Duration};

use clap::Parser;
use runinator_broker::DEFAULT_BROKER_RELAY_PATH;
use runinator_db_cli::DatabaseBackend;
use runinator_models::{errors::SendableError, server_settings::ArchiverSettings};

fn seconds_or_disabled(value: Option<Duration>) -> u64 {
    value.map_or(0, |duration| duration.as_secs())
}

pub fn parse_optional_duration(value: &str) -> Result<Option<Duration>, SendableError> {
    let trimmed = value.trim().to_ascii_lowercase();
    if matches!(trimmed.as_str(), "off" | "none" | "disabled") {
        return Ok(None);
    }
    parse_required_duration(value).map(Some)
}

pub fn parse_required_duration(value: &str) -> Result<Duration, SendableError> {
    let trimmed = value.trim().to_ascii_lowercase();
    if trimmed.is_empty() {
        return Err("duration cannot be empty".into());
    }
    let split_at = trimmed
        .find(|ch: char| !ch.is_ascii_digit())
        .unwrap_or(trimmed.len());
    if split_at == 0 || split_at == trimmed.len() {
        return Err(
            format!("invalid duration '{value}', expected values like 30m, 1h, 90d").into(),
        );
    }
    let amount = trimmed[..split_at].parse::<u64>()?;
    let unit = &trimmed[split_at..];
    let seconds = match unit {
        "s" | "sec" | "secs" | "second" | "seconds" => amount,
        "m" | "min" | "mins" | "minute" | "minutes" => amount * 60,
        "h" | "hr" | "hrs" | "hour" | "hours" => amount * 60 * 60,
        "d" | "day" | "days" => amount * 60 * 60 * 24,
        "w" | "week" | "weeks" => amount * 60 * 60 * 24 * 7,
        _ => return Err(format!("unknown duration unit '{unit}' in '{value}'").into()),
    };
    Ok(Duration::from_secs(seconds.max(1)))
}

#[cfg(test)]
#[path = "config_tests.rs"]
mod tests;

mod cli;
pub use cli::Cli;

mod config;
pub use config::Config;
