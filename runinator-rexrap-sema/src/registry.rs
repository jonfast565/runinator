// the function registry: the unified signature catalog the sema and lowering passes consult. it
// merges the generated intrinsic catalog (from `runinator-workflows`, derived from the rust
// metadata) with the document's user `fn` definitions, and provides named->positional argument
// resolution plus user-function lookups (purity, recursion cycle detection).

use std::collections::HashMap;

use runinator_rexrap_syntax::ast::{Expr, FunctionDef};
use runinator_rexrap_syntax::errors::RexRapError;

/// a callable's signature plus whether it is a user-defined function (vs a native intrinsic) and
/// whether it is effectful. for an intrinsic, effectfulness comes from its metadata `pure` bit; for
/// a user function it is inferred from the body (calls an effectful intrinsic, reads a secret, or
/// transitively calls another effectful function).

/// the function registry merges the generated intrinsic catalog with the document's user functions
/// so it can be stored by value on the lowering and scope structures without lifetime friction.

/// report duplicate function names and user functions that shadow an intrinsic. returns one error
/// per offending definition, anchored to its span.
pub(crate) fn duplicate_errors(functions: &[FunctionDef]) -> Vec<RexRapError> {
    let mut seen: HashMap<&str, ()> = HashMap::new();
    let mut errors = Vec::new();
    for def in functions {
        if runinator_compute::is_known_intrinsic(&def.name) {
            errors.push(RexRapError::semantic(
                def.span,
                format!("function '{}' shadows a built-in intrinsic", def.name),
            ));
        }
        if seen.insert(def.name.as_str(), ()).is_some() {
            errors.push(RexRapError::semantic(
                def.span,
                format!("function '{}' is defined more than once", def.name),
            ));
        }
    }
    errors
}

mod param_sig;
pub(crate) use param_sig::ParamSig;

mod call_sig;
pub(crate) use call_sig::CallSig;

mod function_registry;
pub use function_registry::FunctionRegistry;
