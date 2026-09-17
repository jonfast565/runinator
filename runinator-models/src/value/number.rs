#[allow(unused_imports)]
use super::*;

/// a json number preserving the integer/float distinction, mirroring `serde_json::Number`.
#[derive(Debug, Clone, PartialEq)]
pub struct Number(pub(super) N);

impl Number {
    pub fn is_u64(&self) -> bool {
        matches!(self.0, N::PosInt(_))
    }

    pub fn is_i64(&self) -> bool {
        match self.0 {
            N::NegInt(_) => true,
            N::PosInt(u) => u <= i64::MAX as u64,
            N::Float(_) => false,
        }
    }

    pub fn is_f64(&self) -> bool {
        matches!(self.0, N::Float(_))
    }

    pub fn as_i64(&self) -> Option<i64> {
        match self.0 {
            N::PosInt(u) => i64::try_from(u).ok(),
            N::NegInt(i) => Some(i),
            N::Float(_) => None,
        }
    }

    pub fn as_u64(&self) -> Option<u64> {
        match self.0 {
            N::PosInt(u) => Some(u),
            N::NegInt(i) => u64::try_from(i).ok(),
            N::Float(_) => None,
        }
    }

    pub fn as_f64(&self) -> Option<f64> {
        match self.0 {
            N::PosInt(u) => Some(u as f64),
            N::NegInt(i) => Some(i as f64),
            N::Float(f) => Some(f),
        }
    }

    /// build a number from a float, rejecting non-finite values like `serde_json` does.
    pub fn from_f64(value: f64) -> Option<Number> {
        if value.is_finite() {
            Some(Number(N::Float(value)))
        } else {
            None
        }
    }

    // store non-negative integers as `PosInt` (matching `serde_json`), so that values constructed
    // from signed and unsigned integers compare equal.
    pub(super) fn from_i64(value: i64) -> Self {
        if value >= 0 {
            Number(N::PosInt(value as u64))
        } else {
            Number(N::NegInt(value))
        }
    }
}

impl From<serde_json::Number> for Number {
    fn from(value: serde_json::Number) -> Self {
        if let Some(u) = value.as_u64() {
            Number(N::PosInt(u))
        } else if let Some(i) = value.as_i64() {
            Number(N::NegInt(i))
        } else {
            Number(N::Float(value.as_f64().unwrap_or(0.0)))
        }
    }
}

impl From<Number> for serde_json::Number {
    fn from(value: Number) -> Self {
        match value.0 {
            N::PosInt(u) => serde_json::Number::from(u),
            N::NegInt(i) => serde_json::Number::from(i),
            N::Float(f) => {
                serde_json::Number::from_f64(f).unwrap_or_else(|| serde_json::Number::from(0))
            }
        }
    }
}

impl fmt::Display for Number {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let number: serde_json::Number = self.clone().into();
        fmt::Display::fmt(&number, f)
    }
}

impl Serialize for Number {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        match self.0 {
            N::PosInt(u) => serializer.serialize_u64(u),
            N::NegInt(i) => serializer.serialize_i64(i),
            N::Float(f) => serializer.serialize_f64(f),
        }
    }
}
