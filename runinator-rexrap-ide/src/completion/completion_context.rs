#[allow(unused_imports)]
use super::*;

#[derive(Debug, Clone, Default)]
pub(crate) struct CompletionContext {
    pub(crate) input: RuninatorType,
    pub(crate) bindings: BTreeMap<String, RuninatorType>,
    pub(crate) scoped: BTreeMap<String, RuninatorType>,
    pub(crate) labels: BTreeSet<String>,
    // best-effort output type of the source-order predecessor node, used to type `prev`. `Any`
    // at ambiguous positions (first node, after a control-flow block, inside a nested block).
    pub(crate) prev: RuninatorType,
    // namespace scope derived from the document's `import`s and `fn` definitions, mirroring
    // namespace resolution so bare/aliased completions only offer in-scope names.
    pub(crate) namespace: NamespaceScope,
}
