#[allow(unused_imports)]
use super::*;

pub trait EngineErrors {
    /// every error this engine crate can emit, ordered by code.
    fn error_dictionary() -> &'static [ErrorDescriptor];
}
