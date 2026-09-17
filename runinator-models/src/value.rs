// runinator's own dynamic json-like value, used as the in-memory currency for dynamic state
// (workflow run blobs, node parameters/outputs, expression evaluation, broker payloads) instead of
// reaching for `serde_json::Value` directly. it implements serde's `Serialize`/`Deserialize`, so it
// flows transparently through the http edge, the database text columns, and the broker codec while
// staying byte-compatible json. `serde_json` remains the under-the-hood codec at the actual byte
// boundaries; this type owns the in-memory shape and ergonomics.

use std::collections::BTreeMap;
use std::collections::btree_map;
use std::fmt;

use serde::de::{self, Deserialize, Deserializer, MapAccess, SeqAccess, Visitor};
use serde::ser::{Serialize, SerializeMap, SerializeSeq, Serializer};

/// the reserved object key marking a value as an interpreted closure (a first-class lambda). a
/// closure value is `{ "$closure": { "params": [..], "body": <expr>, "env": {..} } }`. the tag lives
/// here so the type checker (which recognizes closure values) and the expression interpreter (which
/// builds and applies them) share one definition.
pub const CLOSURE_TAG: &str = "$closure";

/// a dynamic json value. object keys are kept sorted (matching `serde_json`'s default ordering) so
/// serialized output stays byte-identical to the previous `serde_json::Value` wire form.
#[derive(Debug, Clone, PartialEq, Default)]
pub enum Value {
    #[default]
    Null,
    Bool(bool),
    Number(Number),
    String(String),
    Array(Vec<Value>),
    Object(Map),
}

#[derive(Debug, Clone, PartialEq)]
enum N {
    PosInt(u64),
    NegInt(i64),
    Float(f64),
}

// value construction and inspection.

impl Value {
    /// look an entry up by string key (objects) or numeric index (arrays).
    pub fn get<I: Index>(&self, index: I) -> Option<&Value> {
        index.index_into(self)
    }

    /// mutable variant of [`Value::get`].
    pub fn get_mut<I: Index>(&mut self, index: I) -> Option<&mut Value> {
        index.index_into_mut(self)
    }

    pub fn is_null(&self) -> bool {
        matches!(self, Value::Null)
    }

    pub fn is_boolean(&self) -> bool {
        matches!(self, Value::Bool(_))
    }

    pub fn is_string(&self) -> bool {
        matches!(self, Value::String(_))
    }

    pub fn is_number(&self) -> bool {
        matches!(self, Value::Number(_))
    }

    pub fn is_array(&self) -> bool {
        matches!(self, Value::Array(_))
    }

    pub fn is_object(&self) -> bool {
        matches!(self, Value::Object(_))
    }

    pub fn is_u64(&self) -> bool {
        matches!(self, Value::Number(n) if n.is_u64())
    }

    pub fn is_i64(&self) -> bool {
        matches!(self, Value::Number(n) if n.is_i64())
    }

    pub fn is_f64(&self) -> bool {
        matches!(self, Value::Number(n) if n.is_f64())
    }

    pub fn as_bool(&self) -> Option<bool> {
        match self {
            Value::Bool(b) => Some(*b),
            _ => None,
        }
    }

    pub fn as_str(&self) -> Option<&str> {
        match self {
            Value::String(s) => Some(s.as_str()),
            _ => None,
        }
    }

    pub fn as_i64(&self) -> Option<i64> {
        match self {
            Value::Number(n) => n.as_i64(),
            _ => None,
        }
    }

    pub fn as_u64(&self) -> Option<u64> {
        match self {
            Value::Number(n) => n.as_u64(),
            _ => None,
        }
    }

    pub fn as_f64(&self) -> Option<f64> {
        match self {
            Value::Number(n) => n.as_f64(),
            _ => None,
        }
    }

    pub fn as_array(&self) -> Option<&Vec<Value>> {
        match self {
            Value::Array(a) => Some(a),
            _ => None,
        }
    }

    pub fn as_array_mut(&mut self) -> Option<&mut Vec<Value>> {
        match self {
            Value::Array(a) => Some(a),
            _ => None,
        }
    }

    pub fn as_object(&self) -> Option<&Map> {
        match self {
            Value::Object(m) => Some(m),
            _ => None,
        }
    }

    pub fn as_object_mut(&mut self) -> Option<&mut Map> {
        match self {
            Value::Object(m) => Some(m),
            _ => None,
        }
    }

    /// resolve a `/`-separated json pointer (RFC 6901 subset), mirroring `serde_json::Value::pointer`.
    pub fn pointer(&self, pointer: &str) -> Option<&Value> {
        if pointer.is_empty() {
            return Some(self);
        }
        if !pointer.starts_with('/') {
            return None;
        }
        pointer
            .split('/')
            .skip(1)
            .map(|token| token.replace("~1", "/").replace("~0", "~"))
            .try_fold(self, |target, token| match target {
                Value::Object(map) => map.get(&token),
                Value::Array(list) => token.parse::<usize>().ok().and_then(|idx| list.get(idx)),
                _ => None,
            })
    }

    /// mutable variant of [`Value::pointer`].
    pub fn pointer_mut(&mut self, pointer: &str) -> Option<&mut Value> {
        if pointer.is_empty() {
            return Some(self);
        }
        if !pointer.starts_with('/') {
            return None;
        }
        pointer
            .split('/')
            .skip(1)
            .map(|token| token.replace("~1", "/").replace("~0", "~"))
            .try_fold(self, |target, token| match target {
                Value::Object(map) => map.get_mut(&token),
                Value::Array(list) => token
                    .parse::<usize>()
                    .ok()
                    .and_then(|idx| list.get_mut(idx)),
                _ => None,
            })
    }
}

// number construction and inspection.

// Object-map API, mirroring the subset of `serde_json::Map` used across the workspace.

// indexing into a value by key or numeric position.

mod private {
    pub trait Sealed {}
    impl Sealed for usize {}
    impl Sealed for str {}
    impl Sealed for String {}
    impl<T> Sealed for &T where T: ?Sized + Sealed {}
}

/// types usable as an index into a [`Value`] via [`Value::get`]. read access only; a missing or
/// type-mismatched index yields `None` rather than panicking, so there is no fallible mutable
/// indexing operator. use [`Value::get_mut`] (or build the value explicitly) to mutate.

// equality against primitives, mirroring `serde_json::Value`'s comparison impls.

impl PartialEq<str> for Value {
    fn eq(&self, other: &str) -> bool {
        self.as_str() == Some(other)
    }
}

impl PartialEq<&str> for Value {
    fn eq(&self, other: &&str) -> bool {
        self.as_str() == Some(*other)
    }
}

impl PartialEq<String> for Value {
    fn eq(&self, other: &String) -> bool {
        self.as_str() == Some(other.as_str())
    }
}

impl PartialEq<bool> for Value {
    fn eq(&self, other: &bool) -> bool {
        self.as_bool() == Some(*other)
    }
}

impl PartialEq<Value> for str {
    fn eq(&self, other: &Value) -> bool {
        other.as_str() == Some(self)
    }
}

impl PartialEq<Value> for &str {
    fn eq(&self, other: &Value) -> bool {
        other.as_str() == Some(*self)
    }
}

macro_rules! partial_eq_signed {
    ($($ty:ty),*) => {
        $(
            impl PartialEq<$ty> for Value {
                fn eq(&self, other: &$ty) -> bool {
                    self.as_i64() == Some(*other as i64)
                }
            }
        )*
    };
}

macro_rules! partial_eq_unsigned {
    ($($ty:ty),*) => {
        $(
            impl PartialEq<$ty> for Value {
                fn eq(&self, other: &$ty) -> bool {
                    self.as_u64() == Some(*other as u64)
                }
            }
        )*
    };
}

macro_rules! partial_eq_float {
    ($($ty:ty),*) => {
        $(
            impl PartialEq<$ty> for Value {
                fn eq(&self, other: &$ty) -> bool {
                    self.as_f64() == Some(*other as f64)
                }
            }
        )*
    };
}

partial_eq_signed!(i8, i16, i32, i64, isize);
partial_eq_unsigned!(u8, u16, u32, u64, usize);
partial_eq_float!(f32, f64);

// `From` conversions for ergonomic construction.

impl From<bool> for Value {
    fn from(value: bool) -> Self {
        Value::Bool(value)
    }
}

impl From<String> for Value {
    fn from(value: String) -> Self {
        Value::String(value)
    }
}

impl From<&str> for Value {
    fn from(value: &str) -> Self {
        Value::String(value.to_owned())
    }
}

impl From<Map> for Value {
    fn from(value: Map) -> Self {
        Value::Object(value)
    }
}

impl<T: Into<Value>> From<Vec<T>> for Value {
    fn from(value: Vec<T>) -> Self {
        Value::Array(value.into_iter().map(Into::into).collect())
    }
}

impl<T: Into<Value>> From<Option<T>> for Value {
    fn from(value: Option<T>) -> Self {
        match value {
            Some(inner) => inner.into(),
            None => Value::Null,
        }
    }
}

macro_rules! from_integer {
    ($($ty:ty => $ctor:expr),* $(,)?) => {
        $(
            impl From<$ty> for Number {
                fn from(value: $ty) -> Self {
                    let ctor: fn($ty) -> Number = $ctor;
                    ctor(value)
                }
            }

            impl From<$ty> for Value {
                fn from(value: $ty) -> Self {
                    Value::Number(Number::from(value))
                }
            }
        )*
    };
}

from_integer! {
    u8 => |value| Number(N::PosInt(value as u64)),
    u16 => |value| Number(N::PosInt(value as u64)),
    u32 => |value| Number(N::PosInt(value as u64)),
    u64 => |value| Number(N::PosInt(value)),
    usize => |value| Number(N::PosInt(value as u64)),
    i8 => |value| Number::from_i64(value as i64),
    i16 => |value| Number::from_i64(value as i64),
    i32 => |value| Number::from_i64(value as i64),
    i64 => Number::from_i64,
    isize => |value| Number::from_i64(value as i64),
}

impl From<f32> for Value {
    fn from(value: f32) -> Self {
        Value::from(value as f64)
    }
}

impl From<f64> for Value {
    fn from(value: f64) -> Self {
        Number::from_f64(value).map_or(Value::Null, Value::Number)
    }
}

// bridges to/from `serde_json` for the byte boundaries (database strings, plugin files) and tests.

impl Value {
    /// decode this dynamic value into a typed `T` through the json codec. the single sanctioned
    /// bridge from `Value` to a typed struct, so domain crates do not reach for `serde_json`
    /// directly (keeping the codec confined to this module).
    pub fn decode<T: serde::de::DeserializeOwned>(&self) -> Result<T, serde_json::Error> {
        serde_json::from_value(self.clone().into())
    }

    /// encode a typed `T` into a dynamic value through the json codec. the inverse of
    /// [`Value::decode`]; use it instead of `serde_json::to_value(..).map(Value::from)`.
    pub fn encode<T: serde::Serialize>(value: &T) -> Result<Value, serde_json::Error> {
        serde_json::to_value(value).map(Value::from)
    }

    /// parse external json text into a dynamic value. the sanctioned text→`Value` boundary (e.g. the
    /// `parse_json` intrinsic), so decoders do not call `serde_json::from_str` directly.
    pub fn from_json_str(text: &str) -> Result<Value, serde_json::Error> {
        serde_json::from_str::<serde_json::Value>(text).map(Value::from)
    }
}

impl From<serde_json::Value> for Value {
    fn from(value: serde_json::Value) -> Self {
        match value {
            serde_json::Value::Null => Value::Null,
            serde_json::Value::Bool(b) => Value::Bool(b),
            serde_json::Value::Number(n) => Value::Number(n.into()),
            serde_json::Value::String(s) => Value::String(s),
            serde_json::Value::Array(list) => {
                Value::Array(list.into_iter().map(Value::from).collect())
            }
            serde_json::Value::Object(object) => Value::Object(Map {
                inner: object
                    .into_iter()
                    .map(|(key, value)| (key, Value::from(value)))
                    .collect(),
            }),
        }
    }
}

impl From<Value> for serde_json::Value {
    fn from(value: Value) -> Self {
        match value {
            Value::Null => serde_json::Value::Null,
            Value::Bool(b) => serde_json::Value::Bool(b),
            Value::Number(n) => serde_json::Value::Number(n.into()),
            Value::String(s) => serde_json::Value::String(s),
            Value::Array(list) => {
                serde_json::Value::Array(list.into_iter().map(serde_json::Value::from).collect())
            }
            Value::Object(map) => serde_json::Value::Object(
                map.inner
                    .into_iter()
                    .map(|(key, value)| (key, serde_json::Value::from(value)))
                    .collect(),
            ),
        }
    }
}

// display renders compact json, matching `serde_json`'s `Display`.

impl fmt::Display for Value {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let rendered = serde_json::to_string(self).map_err(|_| fmt::Error)?;
        f.write_str(&rendered)
    }
}

// serialization: emit the same json shape `serde_json::Value` would.

impl Serialize for Value {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        match self {
            Value::Null => serializer.serialize_unit(),
            Value::Bool(b) => serializer.serialize_bool(*b),
            Value::Number(n) => n.serialize(serializer),
            Value::String(s) => serializer.serialize_str(s),
            Value::Array(list) => {
                let mut seq = serializer.serialize_seq(Some(list.len()))?;
                for element in list {
                    seq.serialize_element(element)?;
                }
                seq.end()
            }
            Value::Object(map) => map.serialize(serializer),
        }
    }
}

// deserialization: accept any json value, mirroring `serde_json::Value`'s visitor.

impl<'de> Deserialize<'de> for Value {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        deserializer.deserialize_any(ValueVisitor)
    }
}

/// build a [`Value`] with the same syntax as `serde_json::json!`.
#[macro_export]
macro_rules! json {
    ($($json:tt)+) => {
        $crate::value::Value::from($crate::__serde_json::json!($($json)+))
    };
}

mod number;
pub use number::Number;

mod map;
pub use map::Map;

mod index;
pub use index::Index;

mod value_visitor;
use value_visitor::ValueVisitor;

mod map_visitor;
use map_visitor::MapVisitor;
