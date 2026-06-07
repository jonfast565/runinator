use serde::{Deserialize, Deserializer, Serialize, Serializer};
use std::fmt;
use std::str::FromStr;

use crate::value::Value;

/// a semantic version (major.minor.patch) for workflow definitions. serializes as a
/// dotted string and tolerates legacy integer/short forms on the wire and from storage.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct SemVer {
    pub major: u64,
    pub minor: u64,
    pub patch: u64,
}

/// the bump level applied when duplicating or versioning a workflow.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SemVerBump {
    Major,
    #[default]
    Minor,
    Patch,
}

impl SemVerBump {
    pub const fn as_str(self) -> &'static str {
        match self {
            SemVerBump::Major => "major",
            SemVerBump::Minor => "minor",
            SemVerBump::Patch => "patch",
        }
    }
}

impl SemVer {
    pub const fn new(major: u64, minor: u64, patch: u64) -> Self {
        Self {
            major,
            minor,
            patch,
        }
    }

    /// bump and reset lower components, mirroring conventional semver increments.
    pub const fn bump(self, level: SemVerBump) -> Self {
        match level {
            SemVerBump::Major => Self::new(self.major + 1, 0, 0),
            SemVerBump::Minor => Self::new(self.major, self.minor + 1, 0),
            SemVerBump::Patch => Self::new(self.major, self.minor, self.patch + 1),
        }
    }
}

impl Default for SemVer {
    fn default() -> Self {
        Self::new(1, 0, 0)
    }
}

impl fmt::Display for SemVer {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "{}.{}.{}", self.major, self.minor, self.patch)
    }
}

impl FromStr for SemVer {
    type Err = String;

    /// parse `major.minor.patch`; missing trailing components default to zero, so `1` and
    /// `1.2` lower to `1.0.0` and `1.2.0` for back-compat with the older integer version.
    fn from_str(value: &str) -> Result<Self, Self::Err> {
        let value = value.trim();
        if value.is_empty() {
            return Err("empty semver".into());
        }
        let mut parts = value.split('.');
        let parse_part = |part: Option<&str>| -> Result<u64, String> {
            match part {
                Some(part) => part
                    .trim()
                    .parse::<u64>()
                    .map_err(|err| format!("invalid semver component '{part}': {err}")),
                None => Ok(0),
            }
        };
        let major = parse_part(parts.next())?;
        let minor = parse_part(parts.next())?;
        let patch = parse_part(parts.next())?;
        if parts.next().is_some() {
            return Err(format!("semver '{value}' has too many components"));
        }
        Ok(Self::new(major, minor, patch))
    }
}

impl Serialize for SemVer {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(&self.to_string())
    }
}

impl<'de> Deserialize<'de> for SemVer {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        // accept a dotted string, or a bare integer carried by legacy payloads/storage.
        let value = Value::deserialize(deserializer)?;
        match value {
            Value::String(text) => text.parse().map_err(serde::de::Error::custom),
            Value::Number(number) => number
                .as_u64()
                .map(|major| SemVer::new(major, 0, 0))
                .ok_or_else(|| {
                    serde::de::Error::custom("version number must be a non-negative integer")
                }),
            other => Err(serde::de::Error::custom(format!(
                "version must be a string or integer, got {other}"
            ))),
        }
    }
}

#[cfg(test)]
mod tests;
