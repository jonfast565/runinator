use std::time::{Duration, SystemTime, UNIX_EPOCH};

use chrono::{DateTime, SecondsFormat, Utc};

/// returns the current wall-clock time as Unix seconds, clamping pre-epoch clocks to zero.
pub fn unix_timestamp_seconds() -> i64 {
    system_time_unix_seconds(SystemTime::now()).unwrap_or(0)
}

/// converts a wall-clock instant to Unix seconds.
pub fn system_time_unix_seconds(value: SystemTime) -> Option<i64> {
    value
        .duration_since(UNIX_EPOCH)
        .ok()
        .map(|duration| duration.as_secs().min(i64::MAX as u64) as i64)
}

/// returns the current wall-clock time as a non-negative Unix nanosecond value.
pub fn unix_timestamp_nanos() -> u128 {
    system_time_unix_nanos(SystemTime::now()).unwrap_or(0)
}

/// converts a wall-clock instant to Unix nanoseconds.
pub fn system_time_unix_nanos(value: SystemTime) -> Option<u128> {
    value
        .duration_since(UNIX_EPOCH)
        .ok()
        .map(|duration| duration.as_nanos())
}

/// converts Unix seconds to UTC, returning `None` when the value is outside chrono's range.
pub fn from_unix_seconds(value: i64) -> Option<DateTime<Utc>> {
    DateTime::from_timestamp(value, 0)
}

/// converts a timestamp expressed in seconds or milliseconds to UTC. Values with at least ten
/// digits are treated as milliseconds, matching the common JavaScript epoch representation.
pub fn from_unix_millis_or_seconds(value: i64) -> Option<DateTime<Utc>> {
    let (seconds, nanos) = if value.unsigned_abs() >= 10_000_000_000 {
        (
            value.div_euclid(1_000),
            value.rem_euclid(1_000) as u32 * 1_000_000,
        )
    } else {
        (value, 0)
    };
    DateTime::from_timestamp(seconds, nanos)
}

/// formats a UTC instant with millisecond precision and a trailing `Z`.
pub fn rfc3339_millis(value: DateTime<Utc>) -> String {
    value.to_rfc3339_opts(SecondsFormat::Millis, true)
}

/// formats a duration compactly for human-facing status output.
pub fn format_duration(duration: Duration) -> String {
    let seconds = duration.as_secs();
    if seconds >= 3600 {
        return format!("{}h {:02}m", seconds / 3600, (seconds % 3600) / 60);
    }
    if seconds >= 60 {
        return format!("{}m {:02}s", seconds / 60, seconds % 60);
    }
    format!("{seconds}s")
}

/// formats whole seconds as a fixed-width uptime clock.
pub fn format_uptime(seconds: u64) -> String {
    let hours = seconds / 3600;
    let minutes = (seconds % 3600) / 60;
    let seconds = seconds % 60;
    format!("{hours:02}:{minutes:02}:{seconds:02}")
}

/// parses a human duration using seconds, minutes, hours, days, or weeks.
pub fn parse_duration(value: &str) -> Result<Duration, std::io::Error> {
    let trimmed = value.trim().to_ascii_lowercase();
    if trimmed.is_empty() {
        return Err(std::io::Error::other("duration cannot be empty"));
    }
    let split_at = trimmed
        .find(|ch: char| !ch.is_ascii_digit())
        .unwrap_or(trimmed.len());
    if split_at == 0 || split_at == trimmed.len() {
        return Err(std::io::Error::other(format!(
            "invalid duration '{value}', expected values like 30m, 1h, 90d"
        )));
    }
    let amount = trimmed[..split_at]
        .parse::<u64>()
        .map_err(std::io::Error::other)?;
    let unit = &trimmed[split_at..];
    let multiplier = match unit {
        "s" | "sec" | "secs" | "second" | "seconds" => 1,
        "m" | "min" | "mins" | "minute" | "minutes" => 60,
        "h" | "hr" | "hrs" | "hour" | "hours" => 60 * 60,
        "d" | "day" | "days" => 60 * 60 * 24,
        "w" | "week" | "weeks" => 60 * 60 * 24 * 7,
        _ => {
            return Err(std::io::Error::other(format!(
                "unknown duration unit '{unit}' in '{value}'"
            )));
        }
    };
    let seconds = amount
        .checked_mul(multiplier)
        .ok_or_else(|| std::io::Error::other("duration is too large"))?;
    Ok(Duration::from_secs(seconds.max(1)))
}

/// parses a duration or one of the conventional disabled values.
pub fn parse_optional_duration(value: &str) -> Result<Option<Duration>, std::io::Error> {
    if matches!(
        value.trim().to_ascii_lowercase().as_str(),
        "off" | "none" | "disabled"
    ) {
        return Ok(None);
    }
    parse_duration(value).map(Some)
}

#[cfg(test)]
#[path = "time_tests.rs"]
mod tests;
