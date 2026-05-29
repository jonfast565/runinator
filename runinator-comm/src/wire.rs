// the single sanctioned (non-protobuf) serialization path across the broker/scheduler/worker
// boundary. callers convert typed structs to and from wire forms here instead of reaching for
// serde_json directly, so domain code stays in terms of structs and `Value` carriers only.

use std::fmt;

use runinator_models::value::Value;
use serde::Serialize;
use serde::de::DeserializeOwned;

/// error raised when a wire conversion fails.
#[derive(Debug)]
pub struct WireError(serde_json::Error);

impl fmt::Display for WireError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "wire codec error: {}", self.0)
    }
}

impl std::error::Error for WireError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        Some(&self.0)
    }
}

impl From<serde_json::Error> for WireError {
    fn from(error: serde_json::Error) -> Self {
        Self(error)
    }
}

/// serialize/deserialize typed structs to wire forms. blanket-implemented for every
/// `Serialize + DeserializeOwned` type, so domain structs gain it for free.
///
/// use `to_wire`/`from_wire` for transport (broker/api strings) and `to_wire_value`/
/// `from_wire_value` when embedding into or reading out of a dynamic `Value` carrier field.
pub trait WireCodec: Serialize + DeserializeOwned + Sized {
    /// serialize to a transport string.
    fn to_wire(&self) -> Result<String, WireError> {
        serde_json::to_string(self).map_err(WireError::from)
    }

    /// deserialize from a transport string.
    fn from_wire(raw: &str) -> Result<Self, WireError> {
        serde_json::from_str(raw).map_err(WireError::from)
    }

    /// serialize into a `Value` carrier (for embedding in a dynamic field).
    fn to_wire_value(&self) -> Result<Value, WireError> {
        serde_json::to_value(self)
            .map(Value::from)
            .map_err(WireError::from)
    }

    /// deserialize out of a `Value` carrier.
    fn from_wire_value(value: &Value) -> Result<Self, WireError> {
        serde_json::from_value(value.clone().into()).map_err(WireError::from)
    }
}

impl<T: Serialize + DeserializeOwned> WireCodec for T {}
