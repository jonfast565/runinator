#[allow(unused_imports)]
use super::*;

/// what one drained stream produced.
#[derive(Debug, Default)]
pub struct Drained {
    pub text: String,
    pub truncated: bool,
}
