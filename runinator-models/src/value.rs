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

/// a json number preserving the integer/float distinction, mirroring `serde_json::Number`.
#[derive(Debug, Clone, PartialEq)]
pub struct Number(N);

#[derive(Debug, Clone, PartialEq)]
enum N {
    PosInt(u64),
    NegInt(i64),
    Float(f64),
}

/// a json object: an ordered string-keyed map of values.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Map {
    inner: BTreeMap<String, Value>,
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

    /// resolve a `/`-separated json pointer (rfc 6901 subset), mirroring `serde_json::Value::pointer`.
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
    fn from_i64(value: i64) -> Self {
        if value >= 0 {
            Number(N::PosInt(value as u64))
        } else {
            Number(N::NegInt(value))
        }
    }
}

// object map api, mirroring the subset of `serde_json::Map` used across the workspace.

impl Map {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with_capacity(_capacity: usize) -> Self {
        Self::default()
    }

    pub fn len(&self) -> usize {
        self.inner.len()
    }

    pub fn is_empty(&self) -> bool {
        self.inner.is_empty()
    }

    pub fn clear(&mut self) {
        self.inner.clear();
    }

    pub fn get(&self, key: &str) -> Option<&Value> {
        self.inner.get(key)
    }

    pub fn get_mut(&mut self, key: &str) -> Option<&mut Value> {
        self.inner.get_mut(key)
    }

    pub fn contains_key(&self, key: &str) -> bool {
        self.inner.contains_key(key)
    }

    pub fn insert(&mut self, key: String, value: Value) -> Option<Value> {
        self.inner.insert(key, value)
    }

    pub fn remove(&mut self, key: &str) -> Option<Value> {
        self.inner.remove(key)
    }

    pub fn entry(&mut self, key: impl Into<String>) -> btree_map::Entry<'_, String, Value> {
        self.inner.entry(key.into())
    }

    pub fn keys(&self) -> btree_map::Keys<'_, String, Value> {
        self.inner.keys()
    }

    pub fn values(&self) -> btree_map::Values<'_, String, Value> {
        self.inner.values()
    }

    pub fn values_mut(&mut self) -> btree_map::ValuesMut<'_, String, Value> {
        self.inner.values_mut()
    }

    pub fn iter(&self) -> btree_map::Iter<'_, String, Value> {
        self.inner.iter()
    }

    pub fn iter_mut(&mut self) -> btree_map::IterMut<'_, String, Value> {
        self.inner.iter_mut()
    }
}

impl FromIterator<(String, Value)> for Map {
    fn from_iter<T: IntoIterator<Item = (String, Value)>>(iter: T) -> Self {
        Self {
            inner: iter.into_iter().collect(),
        }
    }
}

impl Extend<(String, Value)> for Map {
    fn extend<T: IntoIterator<Item = (String, Value)>>(&mut self, iter: T) {
        self.inner.extend(iter);
    }
}

impl IntoIterator for Map {
    type Item = (String, Value);
    type IntoIter = btree_map::IntoIter<String, Value>;

    fn into_iter(self) -> Self::IntoIter {
        self.inner.into_iter()
    }
}

impl<'a> IntoIterator for &'a Map {
    type Item = (&'a String, &'a Value);
    type IntoIter = btree_map::Iter<'a, String, Value>;

    fn into_iter(self) -> Self::IntoIter {
        self.inner.iter()
    }
}

impl<'a> IntoIterator for &'a mut Map {
    type Item = (&'a String, &'a mut Value);
    type IntoIter = btree_map::IterMut<'a, String, Value>;

    fn into_iter(self) -> Self::IntoIter {
        self.inner.iter_mut()
    }
}

impl std::ops::Index<&str> for Map {
    type Output = Value;

    fn index(&self, key: &str) -> &Value {
        static NULL: Value = Value::Null;
        self.inner.get(key).unwrap_or(&NULL)
    }
}

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
pub trait Index: private::Sealed {
    #[doc(hidden)]
    fn index_into<'v>(&self, value: &'v Value) -> Option<&'v Value>;
    #[doc(hidden)]
    fn index_into_mut<'v>(&self, value: &'v mut Value) -> Option<&'v mut Value>;
}

impl Index for usize {
    fn index_into<'v>(&self, value: &'v Value) -> Option<&'v Value> {
        match value {
            Value::Array(list) => list.get(*self),
            _ => None,
        }
    }

    fn index_into_mut<'v>(&self, value: &'v mut Value) -> Option<&'v mut Value> {
        match value {
            Value::Array(list) => list.get_mut(*self),
            _ => None,
        }
    }
}

impl Index for str {
    fn index_into<'v>(&self, value: &'v Value) -> Option<&'v Value> {
        match value {
            Value::Object(map) => map.get(self),
            _ => None,
        }
    }

    fn index_into_mut<'v>(&self, value: &'v mut Value) -> Option<&'v mut Value> {
        match value {
            Value::Object(map) => map.get_mut(self),
            _ => None,
        }
    }
}

impl Index for String {
    fn index_into<'v>(&self, value: &'v Value) -> Option<&'v Value> {
        self.as_str().index_into(value)
    }

    fn index_into_mut<'v>(&self, value: &'v mut Value) -> Option<&'v mut Value> {
        self.as_str().index_into_mut(value)
    }
}

impl<T> Index for &T
where
    T: ?Sized + Index,
{
    fn index_into<'v>(&self, value: &'v Value) -> Option<&'v Value> {
        (**self).index_into(value)
    }

    fn index_into_mut<'v>(&self, value: &'v mut Value) -> Option<&'v mut Value> {
        (**self).index_into_mut(value)
    }
}

impl<I: Index> std::ops::Index<I> for Value {
    type Output = Value;

    fn index(&self, index: I) -> &Value {
        static NULL: Value = Value::Null;
        index.index_into(self).unwrap_or(&NULL)
    }
}

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

// display renders compact json, matching `serde_json`'s `Display`.

impl fmt::Display for Value {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let rendered = serde_json::to_string(self).map_err(|_| fmt::Error)?;
        f.write_str(&rendered)
    }
}

impl fmt::Display for Number {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let number: serde_json::Number = self.clone().into();
        fmt::Display::fmt(&number, f)
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

impl Serialize for Number {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        match self.0 {
            N::PosInt(u) => serializer.serialize_u64(u),
            N::NegInt(i) => serializer.serialize_i64(i),
            N::Float(f) => serializer.serialize_f64(f),
        }
    }
}

impl Serialize for Map {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let mut map = serializer.serialize_map(Some(self.inner.len()))?;
        for (key, value) in &self.inner {
            map.serialize_entry(key, value)?;
        }
        map.end()
    }
}

// deserialization: accept any json value, mirroring `serde_json::Value`'s visitor.

impl<'de> Deserialize<'de> for Value {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        deserializer.deserialize_any(ValueVisitor)
    }
}

struct ValueVisitor;

impl<'de> Visitor<'de> for ValueVisitor {
    type Value = Value;

    fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("any valid json value")
    }

    fn visit_bool<E>(self, value: bool) -> Result<Value, E> {
        Ok(Value::Bool(value))
    }

    fn visit_i64<E>(self, value: i64) -> Result<Value, E> {
        Ok(Value::Number(Number::from(value)))
    }

    fn visit_u64<E>(self, value: u64) -> Result<Value, E> {
        Ok(Value::Number(Number::from(value)))
    }

    fn visit_f64<E>(self, value: f64) -> Result<Value, E> {
        Ok(Number::from_f64(value).map_or(Value::Null, Value::Number))
    }

    fn visit_str<E>(self, value: &str) -> Result<Value, E>
    where
        E: de::Error,
    {
        Ok(Value::String(value.to_owned()))
    }

    fn visit_string<E>(self, value: String) -> Result<Value, E> {
        Ok(Value::String(value))
    }

    fn visit_none<E>(self) -> Result<Value, E> {
        Ok(Value::Null)
    }

    fn visit_some<D: Deserializer<'de>>(self, deserializer: D) -> Result<Value, D::Error> {
        Deserialize::deserialize(deserializer)
    }

    fn visit_unit<E>(self) -> Result<Value, E> {
        Ok(Value::Null)
    }

    fn visit_seq<A: SeqAccess<'de>>(self, mut seq: A) -> Result<Value, A::Error> {
        let mut list = Vec::new();
        while let Some(element) = seq.next_element()? {
            list.push(element);
        }
        Ok(Value::Array(list))
    }

    fn visit_map<A: MapAccess<'de>>(self, mut access: A) -> Result<Value, A::Error> {
        let mut map = Map::new();
        while let Some((key, value)) = access.next_entry()? {
            map.insert(key, value);
        }
        Ok(Value::Object(map))
    }
}

impl<'de> Deserialize<'de> for Map {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        deserializer.deserialize_map(MapVisitor)
    }
}

struct MapVisitor;

impl<'de> Visitor<'de> for MapVisitor {
    type Value = Map;

    fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("a json object")
    }

    fn visit_unit<E>(self) -> Result<Map, E> {
        Ok(Map::new())
    }

    fn visit_map<A: MapAccess<'de>>(self, mut access: A) -> Result<Map, A::Error> {
        let mut map = Map::new();
        while let Some((key, value)) = access.next_entry()? {
            map.insert(key, value);
        }
        Ok(map)
    }
}

/// build a [`Value`] with the same syntax as `serde_json::json!`.
#[macro_export]
macro_rules! json {
    ($($json:tt)+) => {
        $crate::value::Value::from($crate::__serde_json::json!($($json)+))
    };
}
