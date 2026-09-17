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
}

impl IngressPredicate {
    pub fn matches(&self, payload: &Value) -> bool {
        let actual = payload.pointer(&self.pointer);
        match self.operator {
            IngressPredicateOperator::Exists => actual.is_some(),
            IngressPredicateOperator::Equal => actual == self.value.as_ref(),
            IngressPredicateOperator::NotEqual => actual != self.value.as_ref(),
            IngressPredicateOperator::In => self
                .value
                .as_ref()
                .and_then(Value::as_array)
                .is_some_and(|values| actual.is_some_and(|actual| values.contains(actual))),
            IngressPredicateOperator::Contains => match (actual, self.value.as_ref()) {
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
            IngressPredicateOperator::In if !self.value.as_ref().is_some_and(Value::is_array) => {
                Err("an in predicate requires an array value".into())
            }
            _ => Ok(()),
        }
    }
}
