use std::collections::HashMap;
use std::error::Error;
use std::fs;
use std::path::PathBuf;
use std::time::Duration;

use runinator_models::errors::SendableError;

pub(crate) fn normalize_timeout(timeout_secs: i64) -> Duration {
    if timeout_secs <= 0 {
        Duration::from_secs(30)
    } else {
        Duration::from_secs(timeout_secs as u64)
    }
}

pub(crate) fn sanitize_file_stem(input: &str) -> String {
    let mut sanitized = input
        .chars()
        .map(|ch| match ch {
            '<' | '>' | ':' | '"' | '/' | '\\' | '|' | '?' | '*' => '_',
            _ if ch.is_control() => '_',
            _ => ch,
        })
        .collect::<String>();

    sanitized = sanitized
        .trim()
        .trim_matches('.')
        .trim_matches('\'')
        .to_string();

    if sanitized.is_empty() {
        return sanitized;
    }

    const MAX_LEN: usize = 120;
    if sanitized.len() > MAX_LEN {
        sanitized.truncate(MAX_LEN);
    }

    sanitized
}

pub(crate) fn next_available_stem(base: String, counts: &mut HashMap<String, usize>) -> String {
    let counter = counts.entry(base.clone()).or_insert(0usize);
    let stem = if base.is_empty() {
        format!("query_{:02}", *counter + 1)
    } else if *counter == 0 {
        base.clone()
    } else {
        format!("{base}_{:02}", *counter)
    };
    *counter += 1;
    stem
}

pub(crate) fn to_sendable<E>(err: E) -> SendableError
where
    E: Error + Send + Sync + 'static,
{
    Box::new(err)
}

pub(crate) fn file_size(path: &PathBuf) -> Result<i64, SendableError> {
    Ok(fs::metadata(path).map_err(to_sendable)?.len() as i64)
}
