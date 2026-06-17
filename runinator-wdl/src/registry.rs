// the function registry: the unified signature catalog the sema and lowering passes consult. it
// merges the generated intrinsic catalog (from `runinator-workflows`, derived from the rust
// metadata) with the document's user `fn` definitions, and provides named->positional argument
// resolution plus user-function lookups (purity, recursion cycle detection).

use std::collections::HashMap;

use crate::ast::{Expr, FunctionDef};
use crate::errors::WdlError;

/// one parameter of a callable: its name, whether it may be omitted, and (for user functions) its
/// default expression.
#[derive(Clone)]
pub(crate) struct ParamSig {
    pub name: String,
    pub optional: bool,
    pub default: Option<Expr>,
}

/// a callable's signature plus whether it is a user-defined function (vs a native intrinsic) and
/// whether it is effectful. for an intrinsic, effectfulness comes from its metadata `pure` bit; for
/// a user function it is inferred from the body (calls an effectful intrinsic, reads a secret, or
/// transitively calls another effectful function).
pub(crate) struct CallSig {
    pub params: Vec<ParamSig>,
    pub is_user: bool,
    pub effectful: bool,
}

impl CallSig {
    fn required_count(&self) -> usize {
        self.params.iter().filter(|p| !p.optional).count()
    }
}

/// the function registry merges the generated intrinsic catalog with the document's user functions
/// so it can be stored by value on the lowering and scope structures without lifetime friction.
pub(crate) struct FunctionRegistry {
    sigs: HashMap<String, CallSig>,
}

impl FunctionRegistry {
    /// build the registry from the document's functions plus the generated intrinsic catalog. the
    /// caller has already rejected duplicate/shadowing names via `duplicate_errors`.
    pub(crate) fn build(functions: &[FunctionDef]) -> Self {
        let mut sigs = HashMap::new();
        // native intrinsics first; a user function with the same name overrides in the map but is
        // rejected separately by `duplicate_errors`, so this never masks a real intrinsic.
        for action in runinator_workflows::intrinsic_catalog() {
            let params = action
                .parameters
                .iter()
                .map(|param| ParamSig {
                    name: param.name.clone(),
                    optional: !param.required,
                    default: None,
                })
                .collect();
            // an intrinsic's effectfulness is its metadata `pure` bit, the single source of truth
            // shared with the runtime; the front end never re-enumerates pure/effectful names.
            sigs.insert(
                action.function_name.clone(),
                CallSig {
                    params,
                    is_user: false,
                    effectful: !action.pure,
                },
            );
        }
        for def in functions {
            let params = def
                .params
                .iter()
                .map(|param| ParamSig {
                    name: param.name.clone(),
                    optional: param.optional || param.default.is_some(),
                    default: param.default.clone(),
                })
                .collect();
            sigs.insert(
                def.name.clone(),
                CallSig {
                    params,
                    is_user: true,
                    effectful: false,
                },
            );
        }
        let mut registry = Self { sigs };
        registry.compute_effectfulness(functions);
        registry
    }

    /// mark each user function effectful when its body is effectful given the current flags, to a
    /// fixpoint. effectfulness only ever flips false->true, so iterating until stable converges and
    /// naturally propagates transitively (a function calling an effectful function becomes effectful).
    fn compute_effectfulness(&mut self, functions: &[FunctionDef]) {
        loop {
            let mut changed = false;
            for def in functions {
                if self.is_effectful(&def.name) {
                    continue;
                }
                if crate::purity::fn_body_is_effectful(&def.body, self) {
                    if let Some(sig) = self.sigs.get_mut(&def.name) {
                        sig.effectful = true;
                    }
                    changed = true;
                }
            }
            if !changed {
                break;
            }
        }
    }

    /// whether `name` is callable (intrinsic or user function).
    pub(crate) fn knows(&self, name: &str) -> bool {
        self.sigs.contains_key(name) || runinator_workflows::is_known_intrinsic(name)
    }

    /// whether `name` is a user-defined function.
    pub(crate) fn is_user(&self, name: &str) -> bool {
        self.sigs.get(name).is_some_and(|sig| sig.is_user)
    }

    /// whether `name` is a known callable classified as effectful: a non-pure intrinsic (per its
    /// metadata) or a user function whose body is effectful. an unknown name is not "known
    /// effectful" here; callers that must route unknowns to the worker gate on `knows` first.
    pub(crate) fn is_effectful(&self, name: &str) -> bool {
        self.sigs.get(name).is_some_and(|sig| sig.effectful)
    }

    /// resolve a call's positional + named arguments into a single positional list in parameter
    /// order, filling user-function defaults for omitted optionals. errors on unknown/duplicate
    /// keyword names, a positional/keyword collision, a missing required argument, or an omitted
    /// non-trailing argument. a call to an unknown-signature name passes its positional args through
    /// (the unknown name is reported elsewhere).
    pub(crate) fn resolve_args(
        &self,
        name: &str,
        positional: &[Expr],
        named: &[(String, Expr)],
    ) -> Result<Vec<Expr>, String> {
        let Some(sig) = self.sigs.get(name) else {
            if !named.is_empty() {
                return Err(format!(
                    "'{name}' has no known signature, so keyword arguments are not allowed"
                ));
            }
            return Ok(positional.to_vec());
        };
        if positional.len() > sig.params.len() {
            return Err(format!(
                "'{name}' takes at most {} argument(s), got {}",
                sig.params.len(),
                positional.len() + named.len()
            ));
        }
        // slot[i] = the expression bound to parameter i (positional first, then keyword).
        let mut slots: Vec<Option<Expr>> = sig.params.iter().map(|_| None).collect();
        for (index, arg) in positional.iter().enumerate() {
            slots[index] = Some(arg.clone());
        }
        for (key, value) in named {
            let Some(index) = sig.params.iter().position(|param| &param.name == key) else {
                return Err(format!("'{name}' has no parameter named '{key}'"));
            };
            if slots[index].is_some() {
                return Err(format!("argument '{key}' is already supplied to '{name}'"));
            }
            slots[index] = Some(value.clone());
        }
        // fill defaults, then require that every gap is trailing (positional args have no holes).
        let mut resolved = Vec::with_capacity(slots.len());
        for (param, slot) in sig.params.iter().zip(slots.into_iter()) {
            match slot {
                Some(value) => resolved.push(value),
                None => match &param.default {
                    Some(default) => resolved.push(default.clone()),
                    None if param.optional => break,
                    None => {
                        return Err(format!(
                            "'{name}' is missing required argument '{}'",
                            param.name
                        ));
                    }
                },
            }
        }
        // a required/defaulted parameter after the truncation point is an illegal middle gap.
        let provided = resolved.len();
        if let Some(param) = sig.params.get(provided)
            && (!param.optional || param.default.is_some())
        {
            return Err(format!(
                "'{name}' cannot omit non-trailing argument '{}'",
                param.name
            ));
        }
        if provided < sig.required_count() {
            return Err(format!("'{name}' is missing required arguments"));
        }
        Ok(resolved)
    }
}

/// report duplicate function names and user functions that shadow an intrinsic. returns one error
/// per offending definition, anchored to its span.
pub(crate) fn duplicate_errors(functions: &[FunctionDef]) -> Vec<WdlError> {
    let mut seen: HashMap<&str, ()> = HashMap::new();
    let mut errors = Vec::new();
    for def in functions {
        if runinator_workflows::is_known_intrinsic(&def.name) {
            errors.push(WdlError::semantic(
                def.span,
                format!("function '{}' shadows a built-in intrinsic", def.name),
            ));
        }
        if seen.insert(def.name.as_str(), ()).is_some() {
            errors.push(WdlError::semantic(
                def.span,
                format!("function '{}' is defined more than once", def.name),
            ));
        }
    }
    errors
}
