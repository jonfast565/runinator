#[allow(unused_imports)]
use super::*;

pub struct MetaCommand {
    /// the words that select it, e.g. `["run", "workflow"]`.
    pub path: &'static [&'static str],
    /// the full call shape, shown by `:help`.
    pub usage: &'static str,
    pub summary: &'static str,
    /// what that word is, shown when it cannot be completed.
    pub hint: &'static str,
    /// flags that take no value, so `--debug foo` does not read `foo` as the value of `--debug`.
    pub booleans: &'static [&'static str],
}
