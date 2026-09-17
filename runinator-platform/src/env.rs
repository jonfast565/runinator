use std::{env, path::PathBuf, str::FromStr};

/// reads an environment value as a string.
pub fn string(key: &str) -> Option<String> {
    env::var(key).ok()
}

/// reads a non-empty environment value, ignoring surrounding whitespace when deciding whether it
/// is configured but preserving the configured value for callers that need it verbatim.
pub fn non_empty(key: &str) -> Option<String> {
    string(key).filter(|value| !value.trim().is_empty())
}

/// reads and trims an environment value before parsing it.
pub fn parse<T>(key: &str) -> Option<T>
where
    T: FromStr,
{
    parse_value(&string(key)?)
}

/// parses a trimmed string into a caller-selected standard-library type.
pub fn parse_value<T>(value: &str) -> Option<T>
where
    T: FromStr,
{
    value.trim().parse().ok()
}

/// reads and parses an environment value, falling back when it is absent or invalid.
pub fn parse_or<T>(key: &str, default: T) -> T
where
    T: FromStr,
{
    parse(key).unwrap_or(default)
}

/// reads a strictly positive parsed environment value.
pub fn parse_positive<T>(key: &str) -> Option<T>
where
    T: Default + FromStr + PartialOrd,
{
    parse_positive_value(&string(key)?)
}

/// parses a strictly positive value without reading the process environment.
pub fn parse_positive_value<T>(value: &str) -> Option<T>
where
    T: Default + FromStr + PartialOrd,
{
    parse_value(value).filter(|value| *value > T::default())
}

/// reads a strictly positive parsed environment value, falling back when it is absent, invalid, or
/// non-positive.
pub fn parse_positive_or<T>(key: &str, default: T) -> T
where
    T: Default + FromStr + PartialOrd,
{
    parse_positive(key).unwrap_or(default)
}

/// parses the conventional boolean spellings used by process configuration.
pub fn parse_bool(key: &str) -> Option<bool> {
    parse_bool_value(&non_empty(key)?)
}

/// parses a boolean value without reading the process environment.
pub fn parse_bool_value(value: &str) -> Option<bool> {
    match value.trim().to_ascii_lowercase().as_str() {
        "1" | "true" | "yes" | "on" => Some(true),
        "0" | "false" | "no" | "off" => Some(false),
        _ => None,
    }
}

/// reads a boolean environment flag, defaulting to `false` when absent or invalid.
pub fn flag(key: &str) -> bool {
    parse_bool(key).unwrap_or(false)
}

/// reads a boolean environment flag with a caller-provided fallback.
pub fn flag_or(key: &str, default: bool) -> bool {
    parse_bool(key).unwrap_or(default)
}

/// reads a non-empty path-valued environment variable.
pub fn path(key: &str) -> Option<PathBuf> {
    env::var_os(key)
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
}

/// reads a platform-separated path list, returning an empty list when it is not configured.
pub fn paths(key: &str) -> Vec<PathBuf> {
    env::var_os(key)
        .map(|value| env::split_paths(&value).collect())
        .unwrap_or_default()
}
