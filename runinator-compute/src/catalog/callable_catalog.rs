#[allow(unused_imports)]
use super::*;

/// every callable in scope for one compile or one run.
#[derive(Debug, Clone, Default)]
pub struct CallableCatalog {
    pub(super) entries: BTreeMap<String, CallableEntry>,
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

    pub(super) fn insert_intrinsic(
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
