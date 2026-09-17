#[allow(unused_imports)]
use super::*;

/// an object's bytes plus the descriptor they came from. a ranged read carries the resolved range so
/// the caller can build a `Content-Range` without re-deriving it.
#[derive(Debug, Clone)]
pub struct ObjectBytes {
    pub meta: ObjectMeta,
    pub range: Option<ResolvedRange>,
    pub data: Vec<u8>,
}
