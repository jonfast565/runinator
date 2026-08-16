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

/// one resolved callable.
#[derive(Debug, Clone)]
pub struct CallableEntry {
    /// the name a program calls this under.
    pub name: String,
    pub kind: CallableKind,
    /// how far this call's result can travel from where it is computed.
    pub effect: EffectClass,
    /// the typed signature, when one is known.
    pub signature: Option<ActionMetadata>,
    /// accepted argument count `(min, max)`, when known.
    pub arity: Option<(usize, usize)>,
}

impl CallableEntry {
    /// the invocation-ir target this entry compiles to.
    pub fn target(&self) -> CallableTarget {
        match &self.kind {
            CallableKind::Intrinsic | CallableKind::HigherOrder => CallableTarget::Intrinsic {
                name: self.name.clone(),
            },
            CallableKind::Local => CallableTarget::Local {
                name: self.name.clone(),
            },
            CallableKind::Provider { provider, function } => CallableTarget::Provider {
                provider: provider.clone(),
                function: function.clone(),
            },
            CallableKind::Packaged { binding } => CallableTarget::Packaged {
                binding: (**binding).clone(),
            },
        }
    }

    /// whether a call to this can be completed inside the reducer.
    pub fn is_in_process(&self) -> bool {
        self.effect.is_in_process()
    }

    /// whether the argument count is acceptable, when the arity is known.
    pub fn accepts_argc(&self, argc: usize) -> bool {
        match self.arity {
            Some((min, max)) => argc >= min && argc <= max,
            None => true,
        }
    }
}

/// every callable in scope for one compile or one run.
#[derive(Debug, Clone, Default)]
pub struct CallableCatalog {
    entries: BTreeMap<String, CallableEntry>,
}

impl CallableCatalog {
    /// the builtin `std` library alone: pure, local, durable, and higher-order intrinsics.
    ///
    /// signatures come from the same `ActionMetadata` the std provider advertises, so the compiler's
    /// view of an intrinsic cannot drift from the worker's.
    pub fn builtin() -> Self {
        let mut catalog = Self::default();
        for signature in PureIntrinsics::signatures() {
            catalog.insert_intrinsic(signature, EffectClass::Pure, CallableKind::Intrinsic);
        }
        for signature in effectful_signatures() {
            let effect = intrinsic_effect(&signature.function_name);
            catalog.insert_intrinsic(signature, effect, CallableKind::Intrinsic);
        }
        // the assembler's operator intrinsics (`$concat`, `$truthy`, …). they are not author-facing
        // — no completion offers them and no signature describes them — but the catalog is what the
        // vm asks "can this be folded here", and an operator it did not recognize would come back
        // `Unknown` and be dispatched to a worker. every one of them is pure by construction.
        for name in crate::assemble::OPERATOR_INTRINSICS {
            catalog.entries.insert(
                (*name).to_string(),
                CallableEntry {
                    name: (*name).to_string(),
                    kind: CallableKind::Intrinsic,
                    effect: EffectClass::Pure,
                    signature: None,
                    arity: None,
                },
            );
        }
        // the higher-order intrinsics carry no signature of their own: their result depends on the
        // lambda, which is what `intrinsic_result_type` is for. they are structurally pure — a call
        // is only as effectful as the collection and body passed to it.
        for name in HIGHER_ORDER_NAMES {
            catalog.entries.insert(
                (*name).to_string(),
                CallableEntry {
                    name: (*name).to_string(),
                    kind: CallableKind::HigherOrder,
                    effect: EffectClass::Pure,
                    signature: None,
                    arity: intrinsic_arity(name),
                },
            );
        }
        catalog
    }

    fn insert_intrinsic(
        &mut self,
        signature: ActionMetadata,
        effect: EffectClass,
        kind: CallableKind,
    ) {
        let name = signature.function_name.clone();
        let arity = intrinsic_arity(&name);
        self.entries.insert(
            name.clone(),
            CallableEntry {
                name,
                kind,
                effect,
                signature: Some(signature),
                arity,
            },
        );
    }

    /// add the module's own functions.
    ///
    /// `effect` is supplied by the caller because a user function's effect is a property of its
    /// *body*, which the catalog cannot see — the compiler computes it to a fixpoint and tells us.
    pub fn add_local(
        &mut self,
        name: impl Into<String>,
        params: usize,
        effect: EffectClass,
    ) -> &mut Self {
        let name = name.into();
        self.entries.insert(
            name.clone(),
            CallableEntry {
                name,
                kind: CallableKind::Local,
                effect,
                signature: None,
                arity: Some((params, params)),
            },
        );
        self
    }

    /// add every action a provider advertises, under its `provider.function` surface name.
    ///
    /// a provider action is always durable: it is executed by a worker, which is the entire point.
    pub fn add_provider(&mut self, provider: &ProviderMetadata) -> &mut Self {
        for action in &provider.actions {
            let name = format!("{}.{}", provider.name, action.function_name);
            let arity = action_arity(action);
            self.entries.insert(
                name.clone(),
                CallableEntry {
                    name,
                    kind: CallableKind::Provider {
                        provider: provider.name.clone(),
                        function: action.function_name.clone(),
                    },
                    effect: EffectClass::Durable,
                    signature: Some(action.clone()),
                    arity,
                },
            );
        }
        self
    }

    /// add one published packaged export, under its `functions.<package>.<export>` surface name.
    pub fn add_packaged(
        &mut self,
        binding: FunctionBinding,
        signature: Option<ActionMetadata>,
    ) -> &mut Self {
        let name = format!("{}.{}", binding.provider_name(), binding.export_name);
        let arity = signature.as_ref().and_then(action_arity);
        self.entries.insert(
            name.clone(),
            CallableEntry {
                name,
                kind: CallableKind::Packaged {
                    binding: Box::new(binding),
                },
                effect: EffectClass::Durable,
                signature,
                arity,
            },
        );
        self
    }

    /// look up a callable by the name a program calls it under.
    pub fn resolve(&self, name: &str) -> Option<&CallableEntry> {
        self.entries.get(name)
    }

    /// whether the catalog knows this name at all.
    pub fn knows(&self, name: &str) -> bool {
        self.entries.contains_key(name)
    }

    /// the effect class of calling `name`.
    ///
    /// an *unknown* name is [`EffectClass::Unknown`], not `Pure`: refusing to guess is what keeps a
    /// typo or an unregistered provider from being silently folded in the reducer.
    pub fn effect_of(&self, name: &str) -> EffectClass {
        self.entries
            .get(name)
            .map(|entry| entry.effect)
            .unwrap_or(EffectClass::Unknown)
    }

    /// the ir target a called name compiles to.
    ///
    /// an unknown name is treated as a `Local` — a module function the catalog was not told about.
    /// resolving it as a provider instead would silently turn a typo into a broker dispatch, and the
    /// vm's own "unknown function" error names the callee, which is the diagnostic an author wants.
    pub fn target_for(&self, name: &str) -> CallableTarget {
        self.entries
            .get(name)
            .map(CallableEntry::target)
            .unwrap_or_else(|| CallableTarget::Local {
                name: name.to_string(),
            })
    }

    /// every known name, in stable order.
    pub fn names(&self) -> impl Iterator<Item = &str> {
        self.entries.keys().map(String::as_str)
    }

    /// the number of callables in the catalog.
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// bind a call's positional and named arguments into positional order.
    ///
    /// named arguments are matched against the signature's parameter order; a name the signature
    /// does not declare is an error, as is a gap left by an absent required parameter. without a
    /// signature the named arguments are appended in the order written, which is what the untyped
    /// path did before.
    pub fn bind_arguments<T: Clone>(
        &self,
        name: &str,
        positional: &[T],
        named: &[(String, T)],
    ) -> Result<Vec<T>, ArgumentBindError> {
        let Some(entry) = self.entries.get(name) else {
            return Err(ArgumentBindError::UnknownCallable(name.to_string()));
        };
        let Some(signature) = entry.signature.as_ref() else {
            let mut out = positional.to_vec();
            out.extend(named.iter().map(|(_, value)| value.clone()));
            return Ok(out);
        };

        let params = &signature.parameters;
        let mut slots: Vec<Option<T>> = vec![None; params.len().max(positional.len())];
        for (index, value) in positional.iter().enumerate() {
            slots[index] = Some(value.clone());
        }
        for (label, value) in named {
            let Some(index) = params.iter().position(|param| &param.name == label) else {
                return Err(ArgumentBindError::UnknownParameter {
                    callable: name.to_string(),
                    parameter: label.clone(),
                });
            };
            if slots[index].is_some() {
                return Err(ArgumentBindError::DuplicateParameter {
                    callable: name.to_string(),
                    parameter: label.clone(),
                });
            }
            slots[index] = Some(value.clone());
        }

        let mut out = Vec::with_capacity(slots.len());
        for (index, slot) in slots.into_iter().enumerate() {
            match slot {
                Some(value) => out.push(value),
                None => {
                    // a trailing optional simply ends the argument list; a *gap* before a supplied
                    // argument cannot be expressed positionally and is a real error.
                    let required = params.get(index).is_some_and(|param| param.required);
                    if required {
                        return Err(ArgumentBindError::MissingParameter {
                            callable: name.to_string(),
                            parameter: params[index].name.clone(),
                        });
                    }
                    break;
                }
            }
        }
        Ok(out)
    }
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
