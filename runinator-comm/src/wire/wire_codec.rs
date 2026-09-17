#[allow(unused_imports)]
use super::*;

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
