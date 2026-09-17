#[allow(unused_imports)]
use super::*;

/// the names a bare or aliased call may resolve to, gathered from imports and user functions.
#[derive(Debug, Clone, Default)]
pub(crate) struct NamespaceScope {
    /// import alias -> the std module it targets (e.g. `s` -> `strings`). non-std aliases are
    /// omitted because their namespaces have no completable compute members.
    pub(crate) aliases: BTreeMap<String, String>,
    /// intrinsic leaves callable bare because their std module was imported unaliased.
    pub(crate) bare_intrinsics: BTreeSet<String>,
    /// user-defined function names, always callable bare.
    pub(crate) user_fns: BTreeSet<String>,
}
