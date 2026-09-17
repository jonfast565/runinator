use std::{path::PathBuf, time::Duration};

use clap::Parser;
use runinator_broker::DEFAULT_BROKER_RELAY_PATH;
use runinator_db_cli::DatabaseBackend;
use runinator_models::{errors::SendableError, server_settings::ArchiverSettings};
use runinator_platform::time;

fn seconds_or_disabled(value: Option<Duration>) -> u64 {
    value.map_or(0, |duration| duration.as_secs())
}

pub fn parse_optional_duration(value: &str) -> Result<Option<Duration>, SendableError> {
    Ok(time::parse_optional_duration(value)?)
}

pub fn parse_required_duration(value: &str) -> Result<Duration, SendableError> {
    Ok(time::parse_duration(value)?)
}

#[cfg(test)]
#[path = "config_tests.rs"]
mod tests;

mod cli;
pub use cli::Cli;

mod config;
pub use config::Config;
