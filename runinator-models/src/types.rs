use std::collections::BTreeMap;

use serde::{Deserialize, Deserializer, Serialize, Serializer};
use serde_json::{Map, Value};

/// Native Runinator value type metadata.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RuninatorType {
    Null,
    Boolean,
    Integer,
    Number,
    String,
    Array(Box<RuninatorType>),
    Map(Box<RuninatorType>),
    Struct {
        fields: BTreeMap<String, RuninatorType>,
        additional: Option<Box<RuninatorType>>,
    },
    Union(Vec<RuninatorType>),
    Any,
}

impl Default for RuninatorType {
    fn default() -> Self {
        Self::Any
    }
}

impl RuninatorType {
    pub fn array(items: RuninatorType) -> Self {
        Self::Array(Box::new(items))
    }

    pub fn map(values: RuninatorType) -> Self {
        Self::Map(Box::new(values))
    }

    pub fn structure(fields: impl IntoIterator<Item = (impl Into<String>, RuninatorType)>) -> Self {
        Self::Struct {
            fields: fields
                .into_iter()
                .map(|(key, value)| (key.into(), value))
                .collect(),
            additional: None,
        }
    }

    pub fn open_structure(
        fields: impl IntoIterator<Item = (impl Into<String>, RuninatorType)>,
        additional: RuninatorType,
    ) -> Self {
        Self::Struct {
            fields: fields
                .into_iter()
                .map(|(key, value)| (key.into(), value))
                .collect(),
            additional: Some(Box::new(additional)),
        }
    }

    pub fn from_json_schema(schema: &Value) -> Self {
        let Some(object) = schema.as_object() else {
            return Self::Any;
        };
        let schema_type = object.get("type").and_then(Value::as_str);
        if schema_type.is_none() && object.contains_key("properties") {
            return Self::from_object_schema(object);
        }
        match schema_type {
            Some("null") => Self::Null,
            Some("boolean") => Self::Boolean,
            Some("integer") => Self::Integer,
            Some("number") => Self::Number,
            Some("string") => Self::String,
            Some("array") => Self::array(
                object
                    .get("items")
                    .map(Self::from_json_schema)
                    .unwrap_or(Self::Any),
            ),
            Some("object") => Self::from_object_schema(object),
            _ => Self::Any,
        }
    }

    pub fn to_json_schema(&self) -> Value {
        match self {
            Self::Null => serde_json::json!({ "type": "null" }),
            Self::Boolean => serde_json::json!({ "type": "boolean" }),
            Self::Integer => serde_json::json!({ "type": "integer" }),
            Self::Number => serde_json::json!({ "type": "number" }),
            Self::String => serde_json::json!({ "type": "string" }),
            Self::Array(items) => serde_json::json!({
                "type": "array",
                "items": items.to_json_schema(),
            }),
            Self::Map(values) => serde_json::json!({
                "type": "object",
                "additionalProperties": values.to_json_schema(),
            }),
            Self::Struct { fields, additional } => {
                let mut properties = Map::new();
                for (key, ty) in fields {
                    properties.insert(key.clone(), ty.to_json_schema());
                }
                let additional = additional
                    .as_ref()
                    .map(|ty| ty.to_json_schema())
                    .unwrap_or(Value::Bool(false));
                serde_json::json!({
                    "type": "object",
                    "properties": Value::Object(properties),
                    "additionalProperties": additional,
                })
            }
            Self::Union(variants) => serde_json::json!({
                "anyOf": variants.iter().map(Self::to_json_schema).collect::<Vec<_>>(),
            }),
            Self::Any => Value::Bool(true),
        }
    }

    pub fn describe(&self) -> &'static str {
        match self {
            Self::Null => "null",
            Self::Boolean => "boolean",
            Self::Integer => "integer",
            Self::Number => "number",
            Self::String => "string",
            Self::Array(_) => "array",
            Self::Map(_) => "map",
            Self::Struct { .. } => "struct",
            Self::Union(_) => "union",
            Self::Any => "any",
        }
    }

    pub fn field(&self, key: &str) -> Option<&RuninatorType> {
        match self {
            Self::Struct { fields, additional } => fields
                .get(key)
                .or_else(|| additional.as_ref().map(|ty| ty.as_ref())),
            Self::Map(values) => Some(values),
            _ => None,
        }
    }

    pub fn is_numeric(&self) -> bool {
        matches!(self, Self::Integer | Self::Number)
    }

    pub fn is_primitive(&self) -> bool {
        matches!(
            self,
            Self::Boolean | Self::Integer | Self::Number | Self::String
        )
    }

    fn from_object_schema(object: &Map<String, Value>) -> Self {
        let fields: BTreeMap<String, RuninatorType> = object
            .get("properties")
            .and_then(Value::as_object)
            .map(|properties| {
                properties
                    .iter()
                    .map(|(key, value)| (key.clone(), Self::from_json_schema(value)))
                    .collect()
            })
            .unwrap_or_default();
        let additional = match object.get("additionalProperties") {
            Some(Value::Bool(false)) => None,
            Some(Value::Bool(true)) => Some(Box::new(Self::Any)),
            Some(value) => Some(Box::new(Self::from_json_schema(value))),
            None if fields.is_empty() => Some(Box::new(Self::Any)),
            None => None,
        };
        Self::Struct { fields, additional }
    }

    fn from_native_value(value: Value) -> Result<Self, String> {
        if let Value::String(name) = value {
            return Self::from_type_name(&name).ok_or_else(|| format!("unknown type '{name}'"));
        }
        let object = value
            .as_object()
            .ok_or_else(|| "type must be an object or string".to_string())?;
        if object.contains_key("anyOf") {
            let variants = object
                .get("anyOf")
                .and_then(Value::as_array)
                .ok_or_else(|| "anyOf must be an array".to_string())?
                .iter()
                .cloned()
                .map(Self::from_native_value)
                .collect::<Result<Vec<_>, _>>()?;
            return Ok(Self::Union(variants));
        }
        let Some(type_name) = object.get("type").and_then(Value::as_str) else {
            return Ok(Self::from_json_schema(&Value::Object(object.clone())));
        };
        match type_name {
            "null" | "boolean" | "integer" | "number" | "string" | "any" | "json" => {
                Self::from_type_name(type_name).ok_or_else(|| format!("unknown type '{type_name}'"))
            }
            "array" => Ok(Self::array(
                object
                    .get("items")
                    .cloned()
                    .map(Self::from_native_value)
                    .transpose()?
                    .unwrap_or(Self::Any),
            )),
            "map" => Ok(Self::map(
                object
                    .get("values")
                    .or_else(|| object.get("additional"))
                    .cloned()
                    .map(Self::from_native_value)
                    .transpose()?
                    .unwrap_or(Self::Any),
            )),
            "struct" => {
                let fields = object
                    .get("fields")
                    .and_then(Value::as_object)
                    .map(|fields| {
                        fields
                            .iter()
                            .map(|(key, value)| {
                                Self::from_native_value(value.clone()).map(|ty| (key.clone(), ty))
                            })
                            .collect::<Result<BTreeMap<_, _>, _>>()
                    })
                    .transpose()?
                    .unwrap_or_default();
                let additional = match object.get("additional") {
                    Some(Value::Bool(false)) | None => None,
                    Some(Value::Bool(true)) => Some(Box::new(Self::Any)),
                    Some(value) => Some(Box::new(Self::from_native_value(value.clone())?)),
                };
                Ok(Self::Struct { fields, additional })
            }
            "union" => {
                let variants = object
                    .get("variants")
                    .and_then(Value::as_array)
                    .ok_or_else(|| "union variants must be an array".to_string())?
                    .iter()
                    .cloned()
                    .map(Self::from_native_value)
                    .collect::<Result<Vec<_>, _>>()?;
                Ok(Self::Union(variants))
            }
            "object" => Ok(Self::from_json_schema(&Value::Object(object.clone()))),
            other => Err(format!("unknown type '{other}'")),
        }
    }

    fn from_type_name(name: &str) -> Option<Self> {
        match name {
            "null" => Some(Self::Null),
            "boolean" => Some(Self::Boolean),
            "integer" => Some(Self::Integer),
            "number" => Some(Self::Number),
            "string" => Some(Self::String),
            "any" | "json" => Some(Self::Any),
            _ => None,
        }
    }
}

impl Serialize for RuninatorType {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        self.to_native_value().serialize(serializer)
    }
}

impl<'de> Deserialize<'de> for RuninatorType {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        Self::from_native_value(Value::deserialize(deserializer)?).map_err(serde::de::Error::custom)
    }
}

impl RuninatorType {
    fn to_native_value(&self) -> Value {
        match self {
            Self::Null => serde_json::json!({ "type": "null" }),
            Self::Boolean => serde_json::json!({ "type": "boolean" }),
            Self::Integer => serde_json::json!({ "type": "integer" }),
            Self::Number => serde_json::json!({ "type": "number" }),
            Self::String => serde_json::json!({ "type": "string" }),
            Self::Array(items) => serde_json::json!({
                "type": "array",
                "items": items.to_native_value(),
            }),
            Self::Map(values) => serde_json::json!({
                "type": "map",
                "values": values.to_native_value(),
            }),
            Self::Struct { fields, additional } => {
                let mut typed_fields = Map::new();
                for (key, ty) in fields {
                    typed_fields.insert(key.clone(), ty.to_native_value());
                }
                let mut object = Map::from_iter([
                    ("type".into(), Value::String("struct".into())),
                    ("fields".into(), Value::Object(typed_fields)),
                ]);
                if let Some(additional) = additional {
                    object.insert("additional".into(), additional.to_native_value());
                }
                Value::Object(object)
            }
            Self::Union(variants) => serde_json::json!({
                "type": "union",
                "variants": variants.iter().map(Self::to_native_value).collect::<Vec<_>>(),
            }),
            Self::Any => serde_json::json!({ "type": "any" }),
        }
    }
}
