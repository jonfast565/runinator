#[allow(unused_imports)]
use super::*;

/// A deliberately bounded condition over a normalized event payload. Keeping this vocabulary in
/// the model prevents adapters from smuggling executable policy into the control plane.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct IngressPredicate {
    pub pointer: String,
    pub operator: IngressPredicateOperator,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub value: Option<Value>,
    /// Current value of a UUID-bound config reference. This is derived at route evaluation and
    /// intentionally never becomes part of the durable policy snapshot.
    #[serde(skip)]
    pub resolved_value: Option<Value>,
}

impl IngressPredicate {
    /// Returns the authored config setting when this predicate has one direct config reference.
    pub fn config_reference(&self) -> Option<(&str, &str)> {
        let value = self.value.as_ref()?;
        let object = value.as_object()?;
        if object.len() != 1 {
            return None;
        }
        let reference = object.get("$ref")?.as_object()?;
        if reference.len() != 1 {
            return None;
        }
        let parts = reference.get("config")?.as_array()?;
        let (Some(scope), Some(name), None) = (
            parts.first().and_then(Value::as_str),
            parts.get(1).and_then(Value::as_str),
            parts.get(2),
        ) else {
            return None;
        };
        (!scope.is_empty() && !name.is_empty()).then_some((scope, name))
    }

    pub fn matches(&self, payload: &Value) -> bool {
        let actual = payload.pointer(&self.pointer);
        let expected = self.resolved_value.as_ref().or(self.value.as_ref());
        match self.operator {
            IngressPredicateOperator::Exists => actual.is_some(),
            IngressPredicateOperator::Equal => actual == expected,
            IngressPredicateOperator::NotEqual => actual != expected,
            IngressPredicateOperator::In => self
                .resolved_value
                .as_ref()
                .or(self.value.as_ref())
                .and_then(Value::as_array)
                .is_some_and(|values| actual.is_some_and(|actual| values.contains(actual))),
            IngressPredicateOperator::Contains => match (actual, expected) {
                (Some(Value::Array(values)), Some(expected)) => values.contains(expected),
                (Some(Value::String(value)), Some(Value::String(expected))) => {
                    value.contains(expected)
                }
                (Some(Value::Object(values)), Some(Value::String(expected))) => {
                    values.contains_key(expected)
                }
                _ => false,
            },
        }
    }

    pub fn validate(&self) -> Result<(), String> {
        if !self.pointer.is_empty() && !self.pointer.starts_with('/') {
            return Err(format!(
                "ingress predicate pointer '{}' must be empty or start with '/'",
                self.pointer
            ));
        }
        match self.operator {
            IngressPredicateOperator::Exists if self.value.is_some() => {
                Err("an exists predicate must not have a comparison value".into())
            }
            IngressPredicateOperator::Exists => Ok(()),
            _ if self.value.is_none() => Err("an ingress predicate requires a value".into()),
            IngressPredicateOperator::In
                if !self.value.as_ref().is_some_and(Value::is_array)
                    && self.config_reference().is_none() =>
            {
                Err("an in predicate requires an array value".into())
            }
            _ if self.value.as_ref().is_some_and(has_dynamic_value)
                && self.config_reference().is_none() =>
            {
                Err("an ingress predicate may only reference one direct config setting".into())
            }
            _ => Ok(()),
        }
    }
}

fn has_dynamic_value(value: &Value) -> bool {
    match value {
        Value::Array(values) => values.iter().any(has_dynamic_value),
        Value::Object(values) => values
            .iter()
            .any(|(key, value)| key.starts_with('$') || has_dynamic_value(value)),
        _ => false,
    }
}
