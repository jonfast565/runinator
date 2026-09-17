#[allow(unused_imports)]
use super::*;

pub trait ProviderErrors {
    /// every error this provider can emit, ordered by code.
    fn error_dictionary() -> &'static [ErrorDescriptor];
}
