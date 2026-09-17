//! the one place that answers "what can be called, and what does calling it cost".
//!
//! before this, the same vocabulary was spread over five hand-aligned lists (names, signatures,
//! dispatch, module, arity) plus a separate effectful-name list, a higher-order name list, and a
//! per-front-end notion of purity. they drifted, which is why there was a test whose only job was
//! to notice. the catalog assembles all of it once, from the same metadata the worker advertises,
//! and every consumer asks it instead of re-deriving.
//!
//! it deliberately knows nothing about workflow graphs: it maps a *name* to a signature, an arity,
//! and an [`EffectClass`]. what a caller may do with that answer is the caller's rule.

use std::collections::BTreeMap;

use runinator_models::functions::FunctionBinding;
use runinator_models::invocation::{CallableTarget, EffectClass};
use runinator_models::providers::{ActionMetadata, ProviderMetadata};

use crate::compute::{
    EFFECTFUL_INTRINSIC_NAMES, HIGHER_ORDER_NAMES, PureIntrinsics, effectful_signatures,
    intrinsic_arity,
};

/// intrinsics that observe the host but reach nothing outside the process.
///
/// these are split out of [`EFFECTFUL_INTRINSIC_NAMES`] because "not pure" was doing two jobs.
/// `now()` is not reproducible, but making it a durable broker round-trip would cost a dispatch,
/// a persist and a resume to read a clock. they fold in the reducer and the value is *recorded* in
/// the continuation, so a replay, a debugger step, or a shadow cursor sees what the real run saw.
pub const LOCAL_INTRINSIC_NAMES: &[&str] = &["now", "uuid", "env"];

/// what kind of thing a name resolves to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CallableKind {
    /// a `std` library function dispatched through `IntrinsicLibrary`.
    Intrinsic,
    /// a `std` function taking a lambda, evaluated by the vm itself because applying a lambda needs
    /// the evaluator and the context that a plain library call does not have.
    HigherOrder,
    /// a function defined in the module under compilation.
    Local,
    /// a provider action.
    Provider { provider: String, function: String },
    /// a published packaged function.
    Packaged { binding: Box<FunctionBinding> },
}

/// why a call's arguments could not be bound.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ArgumentBindError {
    UnknownCallable(String),
    UnknownParameter { callable: String, parameter: String },
    DuplicateParameter { callable: String, parameter: String },
    MissingParameter { callable: String, parameter: String },
}

impl std::fmt::Display for ArgumentBindError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::UnknownCallable(name) => write!(f, "unknown function '{name}'"),
            Self::UnknownParameter {
                callable,
                parameter,
            } => write!(f, "'{callable}' has no parameter '{parameter}'"),
            Self::DuplicateParameter {
                callable,
                parameter,
            } => write!(f, "'{callable}' got '{parameter}' twice"),
            Self::MissingParameter {
                callable,
                parameter,
            } => write!(f, "'{callable}' is missing required '{parameter}'"),
        }
    }
}

/// the effect class of a builtin intrinsic by name.
pub fn intrinsic_effect(name: &str) -> EffectClass {
    if LOCAL_INTRINSIC_NAMES.contains(&name) {
        return EffectClass::Local;
    }
    if EFFECTFUL_INTRINSIC_NAMES.contains(&name) {
        return EffectClass::Durable;
    }
    if PureIntrinsics::contains(name) || HIGHER_ORDER_NAMES.contains(&name) {
        return EffectClass::Pure;
    }
    EffectClass::Unknown
}

/// the `secret://` scheme a lowered `secret.*` reference becomes.
pub const SECRET_URI_PREFIX: &str = "secret://";

/// whether a lowered value carries a secret reference anywhere inside it.
///
/// this is what keeps the reducer from computing over a secret's *placeholder text*. a
/// `secret.a.b` lowers to the literal string `secret://a/b` and only the worker substitutes the
/// real value, so an in-process `upper(secret.x)` would silently uppercase the placeholder. any
/// program containing one is therefore durable, and runs where secrets resolve.
pub fn contains_secret_reference(value: &runinator_models::value::Value) -> bool {
    use runinator_models::value::Value;
    match value {
        Value::String(text) => text.starts_with(SECRET_URI_PREFIX),
        Value::Array(items) => items.iter().any(contains_secret_reference),
        Value::Object(map) => map.values().any(contains_secret_reference),
        _ => false,
    }
}

// the accepted argument range implied by a signature's parameters.
fn action_arity(action: &ActionMetadata) -> Option<(usize, usize)> {
    let max = action.parameters.len();
    let min = action
        .parameters
        .iter()
        .filter(|param| param.required)
        .count();
    Some((min, max))
}

#[cfg(test)]
#[path = "catalog_tests.rs"]
mod tests;

mod callable_entry;
pub use callable_entry::CallableEntry;

mod callable_catalog;
pub use callable_catalog::CallableCatalog;
