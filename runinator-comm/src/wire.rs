// the single sanctioned (non-protobuf) serialization path across the broker/scheduler/worker
// boundary. callers convert typed structs to and from wire forms here instead of reaching for
// serde_json directly, so domain code stays in terms of structs and `Value` carriers only.

use std::fmt;

use runinator_models::value::Value;
use serde::Serialize;
use serde::de::DeserializeOwned;

/// serialize/deserialize typed structs to wire forms. blanket-implemented for every
/// `Serialize + DeserializeOwned` type, so domain structs gain it for free.
///
/// use `to_wire`/`from_wire` for transport (broker/API strings) and `to_wire_value`/
/// `from_wire_value` when embedding into or reading out of a dynamic `Value` carrier field.
mod wire_error;
pub use wire_error::WireError;

mod wire_codec;
pub use wire_codec::WireCodec;
