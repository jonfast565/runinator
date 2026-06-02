use std::collections::BTreeMap;

use serde::{Deserialize, Deserializer, Serialize, Serializer};

use crate::value::{Map, Value};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RuninatorField {
    pub ty: RuninatorType,
    pub required: bool,
}

impl RuninatorField {
    pub fn required(ty: RuninatorType) -> Self {
        Self { ty, required: true }
    }

    pub fn optional(ty: RuninatorType) -> Self {
        Self {
            ty,
            required: false,
        }
    }

    fn from_native_value(value: Value) -> Result<Self, String> {
        let Some(object) = value.as_object() else {
            return Ok(Self::required(RuninatorType::from_native_value(value)?));
        };
        if object.contains_key("ty") {
            let ty = object
                .get("ty")
                .cloned()
                .map(RuninatorType::from_native_value)
                .transpose()?
                .ok_or_else(|| "field ty is required".to_string())?;
            let required = match object.get("required") {
                Some(Value::Bool(required)) => *required,
                Some(_) => return Err("field required must be a boolean".into()),
                None => true,
            };
            return Ok(Self { ty, required });
        }
        if matches!(object.get("required"), Some(Value::Bool(_))) {
            return Err("field required requires field ty".into());
        }
        Ok(Self::required(RuninatorType::from_native_value(value)?))
    }

    fn to_native_value(&self) -> Value {
        crate::json!({
            "ty": self.ty.to_native_value(),
            "required": self.required,
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TypeViolation {
    pub path: String,
    pub expected: String,
    pub actual: String,
}

impl TypeViolation {
    fn new(path: &[String], expected: impl Into<String>, actual: impl Into<String>) -> Self {
        Self {
            path: format_path(path),
            expected: expected.into(),
            actual: actual.into(),
        }
    }

    pub fn at(path: &[String], expected: impl Into<String>, actual: impl Into<String>) -> Self {
        Self::new(path, expected, actual)
    }

    pub fn message_with_label(&self, label: &str) -> String {
        let label = Self::label_with_path(label, &self.path);
        if self.actual == "missing" {
            return format!("{label} is missing required field");
        }
        if self.actual == "unexpected" {
            return format!("{label} is not allowed");
        }
        format!("{label} expected {}, got {}", self.expected, self.actual)
    }

    pub fn label_with_path(label: &str, path: &str) -> String {
        let path = path.trim_start_matches('$');
        if path.is_empty() {
            return label.to_string();
        }
        if let Some(prefix) = label.strip_suffix('\'') {
            return format!("{prefix}{path}'");
        }
        format!("{label}{path}")
    }
}

impl std::fmt::Display for TypeViolation {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        if self.actual == "missing" {
            return write!(formatter, "{} is missing required field", self.path);
        }
        if self.actual == "unexpected" {
            return write!(formatter, "{} is not allowed", self.path);
        }
        write!(
            formatter,
            "{} expected {}, got {}",
            self.path, self.expected, self.actual
        )
    }
}

impl std::error::Error for TypeViolation {}

/// Native Runinator value type metadata.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub enum RuninatorType {
    Null,
    Boolean,
    Integer,
    Number,
    String,
    Array(Box<RuninatorType>),
    Map(Box<RuninatorType>),
    Struct {
        fields: BTreeMap<String, RuninatorField>,
        additional: Option<Box<RuninatorType>>,
    },
    Union(Vec<RuninatorType>),
    #[default]
    Any,
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
                .map(|(key, value)| (key.into(), RuninatorField::required(value)))
                .collect(),
            additional: None,
        }
    }

    pub fn typed_structure(
        fields: impl IntoIterator<Item = (impl Into<String>, RuninatorField)>,
    ) -> Self {
        Self::Struct {
            fields: fields
                .into_iter()
                .map(|(key, field)| (key.into(), field))
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
                .map(|(key, value)| (key.into(), RuninatorField::required(value)))
                .collect(),
            additional: Some(Box::new(additional)),
        }
    }

    pub fn open_typed_structure(
        fields: impl IntoIterator<Item = (impl Into<String>, RuninatorField)>,
        additional: RuninatorType,
    ) -> Self {
        Self::Struct {
            fields: fields
                .into_iter()
                .map(|(key, field)| (key.into(), field))
                .collect(),
            additional: Some(Box::new(additional)),
        }
    }

    pub fn from_json_schema(schema: &Value) -> Self {
        let Some(object) = schema.as_object() else {
            return Self::Any;
        };
        if let Some(value) = object.get("const") {
            return Self::from_json_value(value);
        }
        if let Some(values) = object.get("enum").and_then(Value::as_array) {
            return Self::dedupe_union(values.iter().map(Self::from_json_value));
        }
        if let Some(variants) = object.get("oneOf").and_then(Value::as_array) {
            return Self::dedupe_union(variants.iter().map(Self::from_json_schema));
        }
        if let Some(variants) = object.get("anyOf").and_then(Value::as_array) {
            return Self::dedupe_union(variants.iter().map(Self::from_json_schema));
        }
        if let Some(items) = object.get("allOf").and_then(Value::as_array) {
            return Self::from_all_of_schema(items);
        }
        if let Some(schema_types) = object.get("type").and_then(Value::as_array) {
            let variants = schema_types
                .iter()
                .filter_map(Value::as_str)
                .map(|type_name| {
                    let mut variant = object.clone();
                    variant.insert("type".into(), Value::String(type_name.into()));
                    Self::from_json_schema(&Value::Object(variant))
                });
            return Self::dedupe_union(variants);
        }
        let schema_type = object.get("type").and_then(Value::as_str);
        if schema_type.is_none() && object.contains_key("properties") {
            return Self::from_object_schema(object);
        }
        let mut ty = match schema_type {
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
        };
        if matches!(object.get("nullable"), Some(Value::Bool(true))) && ty != Self::Null {
            ty = Self::dedupe_union([ty, Self::Null]);
        }
        ty
    }

    pub fn from_json_schema_checked(schema: &Value) -> Result<Self, String> {
        reject_unsupported_schema(schema, "$")?;
        Ok(Self::from_json_schema(schema))
    }

    pub fn to_json_schema(&self) -> Value {
        match self {
            Self::Null => crate::json!({ "type": "null" }),
            Self::Boolean => crate::json!({ "type": "boolean" }),
            Self::Integer => crate::json!({ "type": "integer" }),
            Self::Number => crate::json!({ "type": "number" }),
            Self::String => crate::json!({ "type": "string" }),
            Self::Array(items) => crate::json!({
                "type": "array",
                "items": items.to_json_schema(),
            }),
            Self::Map(values) => crate::json!({
                "type": "object",
                "additionalProperties": values.to_json_schema(),
            }),
            Self::Struct { fields, additional } => {
                let mut properties = Map::new();
                let mut required = Vec::new();
                for (key, field) in fields {
                    properties.insert(key.clone(), field.ty.to_json_schema());
                    if field.required {
                        required.push(Value::String(key.clone()));
                    }
                }
                let additional = additional
                    .as_ref()
                    .map(|ty| ty.to_json_schema())
                    .unwrap_or(Value::Bool(false));
                crate::json!({
                    "type": "object",
                    "properties": Value::Object(properties),
                    "required": Value::Array(required),
                    "additionalProperties": additional,
                })
            }
            Self::Union(variants) => crate::json!({
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
                .map(|field| &field.ty)
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

    pub fn validate_value(&self, value: &Value) -> Result<(), TypeViolation> {
        self.validate_value_at(value, &mut Vec::new())
    }

    pub fn validate_assignable_to(&self, expected: &Self) -> Result<(), TypeViolation> {
        self.validate_assignable_to_at(expected, &mut Vec::new())
    }

    fn validate_value_at(
        &self,
        value: &Value,
        path: &mut Vec<String>,
    ) -> Result<(), TypeViolation> {
        match self {
            Self::Any => Ok(()),
            Self::Null if value.is_null() => Ok(()),
            Self::Boolean if value.is_boolean() => Ok(()),
            Self::Integer if value.as_i64().is_some() || value.as_u64().is_some() => Ok(()),
            Self::Number if value.is_number() => Ok(()),
            Self::String if value.is_string() => Ok(()),
            Self::Array(item_type) => {
                let Some(items) = value.as_array() else {
                    return Err(TypeViolation::new(
                        path,
                        self.describe(),
                        actual_type(value),
                    ));
                };
                for (index, item) in items.iter().enumerate() {
                    path.push(format!("[{index}]"));
                    item_type.validate_value_at(item, path)?;
                    path.pop();
                }
                Ok(())
            }
            Self::Map(value_type) => {
                let Some(object) = value.as_object() else {
                    return Err(TypeViolation::new(
                        path,
                        self.describe(),
                        actual_type(value),
                    ));
                };
                for (key, nested) in object {
                    path.push(key.clone());
                    value_type.validate_value_at(nested, path)?;
                    path.pop();
                }
                Ok(())
            }
            Self::Struct { fields, additional } => {
                let Some(object) = value.as_object() else {
                    return Err(TypeViolation::new(
                        path,
                        self.describe(),
                        actual_type(value),
                    ));
                };
                for (key, field) in fields {
                    if !object.contains_key(key) {
                        if field.required {
                            path.push(key.clone());
                            let violation =
                                TypeViolation::new(path, field.ty.describe(), "missing");
                            path.pop();
                            return Err(violation);
                        }
                        continue;
                    }
                    path.push(key.clone());
                    field.ty.validate_value_at(&object[key], path)?;
                    path.pop();
                }
                for (key, nested) in object {
                    if fields.contains_key(key) {
                        continue;
                    }
                    path.push(key.clone());
                    match additional {
                        Some(value_type) => value_type.validate_value_at(nested, path)?,
                        None => {
                            let violation =
                                TypeViolation::new(path, "no additional fields", "unexpected");
                            path.pop();
                            return Err(violation);
                        }
                    }
                    path.pop();
                }
                Ok(())
            }
            Self::Union(variants) => {
                let mut best_violation = None;
                for variant in variants {
                    match variant.validate_value_at(value, &mut path.clone()) {
                        Ok(()) => return Ok(()),
                        Err(violation) => {
                            best_violation = Some(best_type_violation(best_violation, violation));
                        }
                    }
                }
                Err(best_violation.unwrap_or_else(|| {
                    TypeViolation::new(path, self.describe(), actual_type(value))
                }))
            }
            expected => Err(TypeViolation::new(
                path,
                expected.describe(),
                actual_type(value),
            )),
        }
    }

    fn validate_assignable_to_at(
        &self,
        expected: &Self,
        path: &mut Vec<String>,
    ) -> Result<(), TypeViolation> {
        if self == expected || matches!(expected, Self::Any) {
            return Ok(());
        }
        if matches!((self, expected), (Self::Integer, Self::Number)) {
            return Ok(());
        }
        match (self, expected) {
            (Self::Array(actual), Self::Array(expected)) => {
                path.push("[*]".into());
                let result = actual.validate_assignable_to_at(expected, path);
                path.pop();
                result
            }
            (Self::Map(actual), Self::Map(expected)) => {
                path.push("*".into());
                let result = actual.validate_assignable_to_at(expected, path);
                path.pop();
                result
            }
            (
                Self::Struct {
                    fields: actual_fields,
                    additional: actual_additional,
                },
                Self::Struct {
                    fields: expected_fields,
                    additional: expected_additional,
                },
            ) => {
                for (key, expected_field) in expected_fields {
                    let actual_field = actual_fields.get(key);
                    if actual_field.is_none() && expected_field.required {
                        path.push(key.clone());
                        let violation =
                            TypeViolation::new(path, expected_field.ty.describe(), "missing");
                        path.pop();
                        return Err(violation);
                    }
                    let Some(actual_field) = actual_field else {
                        continue;
                    };
                    if expected_field.required && !actual_field.required {
                        path.push(key.clone());
                        let violation =
                            TypeViolation::new(path, expected_field.ty.describe(), "missing");
                        path.pop();
                        return Err(violation);
                    }
                    path.push(key.clone());
                    let result = actual_field
                        .ty
                        .validate_assignable_to_at(&expected_field.ty, path);
                    path.pop();
                    result?;
                }
                for (key, actual_field) in actual_fields {
                    if expected_fields.contains_key(key) {
                        continue;
                    }
                    path.push(key.clone());
                    match expected_additional {
                        Some(expected) => {
                            let result = actual_field.ty.validate_assignable_to_at(expected, path);
                            path.pop();
                            result?;
                        }
                        None => {
                            let violation =
                                TypeViolation::new(path, "no additional fields", "unexpected");
                            path.pop();
                            return Err(violation);
                        }
                    }
                }
                if let Some(actual_additional) = actual_additional {
                    let Some(expected_additional) = expected_additional else {
                        path.push("*".into());
                        let violation =
                            TypeViolation::new(path, "no additional fields", "unexpected");
                        path.pop();
                        return Err(violation);
                    };
                    path.push("*".into());
                    let result =
                        actual_additional.validate_assignable_to_at(expected_additional, path);
                    path.pop();
                    result?;
                }
                Ok(())
            }
            (Self::Struct { fields, additional }, Self::Map(expected)) => {
                for (key, actual_field) in fields {
                    path.push(key.clone());
                    let result = actual_field.ty.validate_assignable_to_at(expected, path);
                    path.pop();
                    result?;
                }
                if let Some(additional) = additional {
                    path.push("*".into());
                    let result = additional.validate_assignable_to_at(expected, path);
                    path.pop();
                    result?;
                }
                Ok(())
            }
            (Self::Map(actual), Self::Struct { fields, .. }) => {
                for (key, expected_field) in fields {
                    if expected_field.required {
                        path.push(key.clone());
                        let violation =
                            TypeViolation::new(path, expected_field.ty.describe(), "missing");
                        path.pop();
                        return Err(violation);
                    }
                    path.push(key.clone());
                    let result = actual.validate_assignable_to_at(&expected_field.ty, path);
                    path.pop();
                    result?;
                }
                Ok(())
            }
            (Self::Union(variants), expected) => {
                let mut best_violation = None;
                for variant in variants {
                    match variant.validate_assignable_to_at(expected, &mut path.clone()) {
                        Ok(()) => {}
                        Err(violation) => {
                            best_violation = Some(best_type_violation(best_violation, violation));
                        }
                    }
                }
                best_violation.map_or(Ok(()), Err)
            }
            (actual, Self::Union(variants)) => {
                let mut best_violation = None;
                for variant in variants {
                    match actual.validate_assignable_to_at(variant, &mut path.clone()) {
                        Ok(()) => return Ok(()),
                        Err(violation) => {
                            best_violation = Some(best_type_violation(best_violation, violation));
                        }
                    }
                }
                Err(best_violation.unwrap_or_else(|| {
                    TypeViolation::new(path, expected.describe(), actual.describe())
                }))
            }
            (actual, expected) => Err(TypeViolation::new(
                path,
                expected.describe(),
                actual.describe(),
            )),
        }
    }

    fn from_object_schema(object: &Map) -> Self {
        let required = object
            .get("required")
            .and_then(Value::as_array)
            .map(|items| {
                items
                    .iter()
                    .filter_map(Value::as_str)
                    .map(str::to_string)
                    .collect::<std::collections::BTreeSet<_>>()
            })
            .unwrap_or_default();
        let fields: BTreeMap<String, RuninatorField> = object
            .get("properties")
            .and_then(Value::as_object)
            .map(|properties| {
                properties
                    .iter()
                    .map(|(key, value)| {
                        (
                            key.clone(),
                            RuninatorField {
                                ty: Self::from_json_schema(value),
                                required: required.contains(key),
                            },
                        )
                    })
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

    fn from_all_of_schema(items: &[Value]) -> Self {
        let mut merged_fields = BTreeMap::new();
        let mut merged_additional = Some(Box::new(Self::Any));
        let mut saw_struct = false;
        let mut non_structs = Vec::new();

        for item in items {
            match Self::from_json_schema(item) {
                Self::Struct { fields, additional } => {
                    saw_struct = true;
                    merged_fields.extend(fields);
                    if additional.is_none() {
                        merged_additional = None;
                    } else if matches!(merged_additional.as_deref(), Some(Self::Any)) {
                        merged_additional = additional;
                    }
                }
                Self::Any => {}
                ty => non_structs.push(ty),
            }
        }

        if saw_struct && non_structs.is_empty() {
            return Self::Struct {
                fields: merged_fields,
                additional: merged_additional,
            };
        }
        Self::dedupe_union(non_structs)
    }

    /// infer a schema type from a concrete value, used when no schema is declared for a config
    /// slot. objects become open structs whose known fields are optional and typed (so the shape
    /// can evolve while known fields still type-check), arrays become arrays over the union of
    /// their element types, and primitives map to their narrowest type.
    pub fn infer_from_value(value: &Value) -> Self {
        match value {
            Value::Null => Self::Null,
            Value::Bool(_) => Self::Boolean,
            Value::Number(number) if number.as_i64().is_some() || number.as_u64().is_some() => {
                Self::Integer
            }
            Value::Number(_) => Self::Number,
            Value::String(_) => Self::String,
            Value::Array(items) => {
                Self::array(Self::dedupe_union(items.iter().map(Self::infer_from_value)))
            }
            Value::Object(object) => Self::Struct {
                fields: object
                    .iter()
                    .map(|(key, value)| {
                        (
                            key.clone(),
                            RuninatorField::optional(Self::infer_from_value(value)),
                        )
                    })
                    .collect(),
                additional: Some(Box::new(Self::Any)),
            },
        }
    }

    fn from_json_value(value: &Value) -> Self {
        match value {
            Value::Null => Self::Null,
            Value::Bool(_) => Self::Boolean,
            Value::Number(number) if number.as_i64().is_some() || number.as_u64().is_some() => {
                Self::Integer
            }
            Value::Number(_) => Self::Number,
            Value::String(_) => Self::String,
            Value::Array(items) => {
                Self::array(Self::dedupe_union(items.iter().map(Self::from_json_value)))
            }
            Value::Object(_) => Self::map(Self::Any),
        }
    }

    fn dedupe_union(types: impl IntoIterator<Item = Self>) -> Self {
        let mut variants = Vec::new();
        for ty in types {
            if ty == Self::Any {
                return Self::Any;
            }
            if !variants.contains(&ty) {
                variants.push(ty);
            }
        }
        match variants.len() {
            0 => Self::Any,
            1 => variants.pop().unwrap_or(Self::Any),
            _ => Self::Union(variants),
        }
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
            if variants.is_empty() {
                return Err("union variants must not be empty".into());
            }
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
                                RuninatorField::from_native_value(value.clone())
                                    .map(|field| (key.clone(), field))
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
                if variants.is_empty() {
                    return Err("union variants must not be empty".into());
                }
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
            Self::Null => crate::json!({ "type": "null" }),
            Self::Boolean => crate::json!({ "type": "boolean" }),
            Self::Integer => crate::json!({ "type": "integer" }),
            Self::Number => crate::json!({ "type": "number" }),
            Self::String => crate::json!({ "type": "string" }),
            Self::Array(items) => crate::json!({
                "type": "array",
                "items": items.to_native_value(),
            }),
            Self::Map(values) => crate::json!({
                "type": "map",
                "values": values.to_native_value(),
            }),
            Self::Struct { fields, additional } => {
                let mut typed_fields = Map::new();
                for (key, field) in fields {
                    typed_fields.insert(key.clone(), field.to_native_value());
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
            Self::Union(variants) => crate::json!({
                "type": "union",
                "variants": variants.iter().map(Self::to_native_value).collect::<Vec<_>>(),
            }),
            Self::Any => crate::json!({ "type": "any" }),
        }
    }
}

fn actual_type(value: &Value) -> &'static str {
    match value {
        Value::Null => "null",
        Value::Bool(_) => "boolean",
        Value::Number(number) if number.as_i64().is_some() || number.as_u64().is_some() => {
            "integer"
        }
        Value::Number(_) => "number",
        Value::String(_) => "string",
        Value::Array(_) => "array",
        Value::Object(_) => "object",
    }
}

fn reject_unsupported_schema(schema: &Value, path: &str) -> Result<(), String> {
    let Some(object) = schema.as_object() else {
        return Err(format!("{path} must be a JSON Schema object"));
    };
    for key in [
        "patternProperties",
        "not",
        "if",
        "then",
        "else",
        "dependentSchemas",
        "dependencies",
    ] {
        if object.contains_key(key) {
            return Err(format!("{path}.{key} is not supported"));
        }
    }
    if let Some(schema_type) = object.get("type") {
        match schema_type {
            Value::String(name) => reject_unknown_schema_type(name, path)?,
            Value::Array(items) => {
                if items.is_empty() {
                    return Err(format!("{path}.type must not be empty"));
                }
                for (index, item) in items.iter().enumerate() {
                    let Some(name) = item.as_str() else {
                        return Err(format!("{path}.type[{index}] must be a string"));
                    };
                    reject_unknown_schema_type(name, &format!("{path}.type[{index}]"))?;
                }
            }
            _ => return Err(format!("{path}.type must be a string or string array")),
        }
    }
    for key in ["oneOf", "anyOf", "allOf"] {
        if let Some(items) = object.get(key) {
            let Some(items) = items.as_array() else {
                return Err(format!("{path}.{key} must be an array"));
            };
            if items.is_empty() {
                return Err(format!("{path}.{key} must not be empty"));
            }
            for (index, item) in items.iter().enumerate() {
                reject_unsupported_schema(item, &format!("{path}.{key}[{index}]"))?;
            }
        }
    }
    if let Some(items) = object.get("items") {
        if items.is_array() {
            return Err(format!("{path}.items tuple arrays are not supported"));
        }
        reject_unsupported_schema(items, &format!("{path}.items"))?;
    }
    if let Some(properties) = object.get("properties") {
        let Some(properties) = properties.as_object() else {
            return Err(format!("{path}.properties must be an object"));
        };
        for (key, value) in properties {
            reject_unsupported_schema(value, &format!("{path}.properties.{key}"))?;
        }
    }
    if let Some(additional) = object.get("additionalProperties") {
        match additional {
            Value::Bool(_) => {}
            Value::Object(_) => {
                reject_unsupported_schema(additional, &format!("{path}.additionalProperties"))?
            }
            _ => {
                return Err(format!(
                    "{path}.additionalProperties must be a boolean or schema"
                ));
            }
        }
    }
    Ok(())
}

fn reject_unknown_schema_type(name: &str, path: &str) -> Result<(), String> {
    match name {
        "null" | "boolean" | "integer" | "number" | "string" | "array" | "object" => Ok(()),
        other => Err(format!(
            "{path} uses unsupported JSON Schema type '{other}'"
        )),
    }
}

fn format_path(path: &[String]) -> String {
    if path.is_empty() {
        return "$".into();
    }
    let mut rendered = String::from("$");
    for segment in path {
        if segment.starts_with('[') {
            rendered.push_str(segment);
        } else {
            rendered.push('.');
            rendered.push_str(segment);
        }
    }
    rendered
}

fn best_type_violation(current: Option<TypeViolation>, candidate: TypeViolation) -> TypeViolation {
    let Some(current) = current else {
        return candidate;
    };
    if path_specificity(&candidate.path) > path_specificity(&current.path) {
        return candidate;
    }
    current
}

fn path_specificity(path: &str) -> usize {
    path.matches('.').count() + path.matches('[').count()
}
