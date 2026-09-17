#[allow(unused_imports)]
use super::*;

#[derive(Debug, Clone, PartialEq)]
pub struct RuninatorField {
    pub ty: RuninatorType,
    pub required: bool,
    /// an optional default value for the field. for workflow input fields this may be a lowered
    /// expression (`{"$ref":...}`, `{"$concat":...}`, a `secret://` string, or a literal) that is
    /// evaluated against the run context when the field is omitted.
    pub default: Option<Value>,
}

impl RuninatorField {
    pub fn required(ty: RuninatorType) -> Self {
        Self {
            ty,
            required: true,
            default: None,
        }
    }

    pub fn optional(ty: RuninatorType) -> Self {
        Self {
            ty,
            required: false,
            default: None,
        }
    }

    /// attach a default value; a defaulted field is treated as optional since the default fills it.
    pub fn with_default(mut self, default: Value) -> Self {
        self.default = Some(default);
        self.required = false;
        self
    }

    pub(super) fn from_native_value(value: Value) -> Result<Self, String> {
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
            let default = object.get("default").cloned();
            return Ok(Self {
                ty,
                required,
                default,
            });
        }
        if matches!(object.get("required"), Some(Value::Bool(_))) {
            return Err("field required requires field ty".into());
        }
        Ok(Self::required(RuninatorType::from_native_value(value)?))
    }

    pub(super) fn to_native_value(&self) -> Value {
        let mut object = Map::from_iter([
            ("ty".into(), self.ty.to_native_value()),
            ("required".into(), Value::Bool(self.required)),
        ]);
        if let Some(default) = &self.default {
            object.insert("default".into(), default.clone());
        }
        Value::Object(object)
    }
}
