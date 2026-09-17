#[allow(unused_imports)]
use super::*;

/// a header `watch <cond> -> <target>` guard: when `cond` holds, the run jumps to `handler`.
#[derive(Debug, Clone, PartialEq)]
pub struct WatchDecl {
    pub cond: Cond,
    pub handler: Target,
}
