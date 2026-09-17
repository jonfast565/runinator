#[allow(unused_imports)]
use super::*;

pub trait IntrinsicLibrary {
    /// invoke `name` with already-evaluated `args`.
    fn call(&self, name: &str, args: &[Value]) -> Result<Value, WorkflowValidationError>;
    /// whether the library exposes `name`.
    fn knows(&self, name: &str) -> bool;
    /// whether `name` is pure (reducer-evaluable).
    fn is_pure(&self, name: &str) -> bool;
}
