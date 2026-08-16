//! what a yielded call asks orchestration to do, and what comes back.

use super::*;

/// a call the vm could not make in process.
///
/// this is the whole request: the target, the arguments already evaluated, and the policy that
/// governs the attempt. orchestration turns it into a durable call record plus an action dispatch;
/// it never needs to look back into the program to do so.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct InvocationEffect {
    /// this call's ordinal within the invocation, which names it for dedupe and attribution.
    pub sequence: i64,
    pub target: CallableTarget,
    /// arguments in call order, already evaluated.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub args: Vec<Value>,
    /// named-argument labels aligned with the trailing entries of `args`.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub names: Vec<String>,
    /// the effective policy: the node's defaults with any call-site overrides applied.
    #[serde(default)]
    pub policy: CallPolicy,
}

impl InvocationEffect {
    /// the arguments as a parameter object, using named labels where the author gave them and
    /// positional `arg0`, `arg1`, … otherwise.
    pub fn to_parameters(&self) -> Value {
        let mut map = crate::value::Map::new();
        let unnamed = self.args.len().saturating_sub(self.names.len());
        for (index, value) in self.args.iter().enumerate() {
            let key = match index.checked_sub(unnamed).and_then(|at| self.names.get(at)) {
                Some(name) => name.clone(),
                None => format!("arg{index}"),
            };
            map.insert(key, value.clone());
        }
        Value::Object(map)
    }
}

/// the outcome of a yielded call, handed back to the vm to resume with.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum InvocationEffectResult {
    /// the call produced a value.
    Ok { value: Value },
    /// the call failed after exhausting its own retry policy.
    Failed { message: String },
}

impl InvocationEffectResult {
    pub fn ok(value: Value) -> Self {
        Self::Ok { value }
    }

    pub fn failed(message: impl Into<String>) -> Self {
        Self::Failed {
            message: message.into(),
        }
    }
}
