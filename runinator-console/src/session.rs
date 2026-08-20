//! how a cell sees the cells before it.

use runinator_models::value::{Map, Value};
use uuid::Uuid;

/// what an author writes to reach an earlier cell's result: `params.load.rows`.
///
/// `params` rather than a console-only root like `cells`, and the reason is decisive: a bare dotted
/// path in a rexrap expression means *node output*, so `cells.load` lowers to a reference to a node
/// called `cells`. a genuinely new root would have to be taught to the lowerer, the type checker,
/// the ide, and the decompiler. `params` is already the root for "the values this run was given",
/// which is exactly what a console scope is.
///
/// it is still a namespace, so the property that mattered survives: a cell labelled `config` binds
/// to `params.config` and cannot shadow the real `config` root, a provider name, or `secret`.
///
/// it is also what makes the two execution paths agree — a scratch workflow is started with the
/// scope as its run parameters, so `params.load` means one thing however the cell ran.
pub const CELL_SCOPE: &str = "params";

/// the key the evaluator reads that scope under, which is **not** [`CELL_SCOPE`].
///
/// surface `params.x` lowers to `{"$ref": {"params": ["x"]}}`, and the evaluator resolves that
/// against a context keyed `input`. the surface name and the context key are simply different names
/// for the same thing, and building the context under the surface name yields a valid-looking
/// expression that fails to resolve — which is exactly the mistake this constant exists to stop
/// anyone repeating.
pub const CONTEXT_ROOT: &str = "input";

/// the binding name a cell's result is stored under.
///
/// an author-supplied label when the cell has one, else `cell_<n>` — positional, so an unlabelled
/// cell is still referenceable without forcing every cell to be named.
pub fn cell_binding_name(label: Option<&str>, position: i64) -> String {
    match label.map(str::trim).filter(|label| !label.is_empty()) {
        Some(label) => label.to_string(),
        None => format!("cell_{position}"),
    }
}

/// the workflow name a scratch cell run compiles under.
///
/// scoped by session and cell so two sessions running the same cell text do not collide, and so a
/// scratch workflow is traceable back to the cell that made it.
pub fn scratch_workflow_name(session_id: Uuid, cell_id: Uuid) -> String {
    format!("console.{}.{}", session_id.simple(), cell_id.simple())
}

/// the evaluation context one cell runs against.
///
/// built fresh from the session's stored bindings on every run rather than accumulated in memory:
/// a console session outlives any one replica, and an in-memory accumulation would give different
/// answers depending on which replica served the request.
#[derive(Debug, Clone, Default)]
pub struct ConsoleContext {
    bindings: Map,
}

impl ConsoleContext {
    pub fn new() -> Self {
        Self::default()
    }

    /// record one cell's result under its binding name.
    pub fn bind(&mut self, name: &str, value: Value) {
        self.bindings.insert(name.to_string(), value);
    }

    /// the context value an expression is resolved against.
    pub fn as_value(&self) -> Value {
        let mut context = Map::new();
        context.insert(
            CONTEXT_ROOT.to_string(),
            Value::Object(self.bindings.clone()),
        );
        Value::Object(context)
    }

    /// the bindings alone, for use as a scratch workflow's run parameters.
    ///
    /// unwrapped because the reducer nests run parameters under `params` itself; wrapping here too
    /// would make a cell reach `params.params.load`.
    pub fn as_parameters(&self) -> Value {
        Value::Object(self.bindings.clone())
    }

    pub fn is_empty(&self) -> bool {
        self.bindings.is_empty()
    }

    pub fn names(&self) -> Vec<String> {
        self.bindings.keys().cloned().collect()
    }
}

#[cfg(test)]
#[path = "session_tests.rs"]
mod tests;
