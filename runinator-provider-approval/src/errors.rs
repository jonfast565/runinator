use runinator_models::errors::{ErrorDescriptor, ProviderErrors};

use crate::ApprovalProvider;

// numbered error dictionary for the approval provider.
pub(crate) const INVALID_PARAMS: ErrorDescriptor = ErrorDescriptor::new(
    "APPROVAL001",
    "approval.invalid_params",
    "Invalid parameters",
);

pub(crate) const DICTIONARY: &[ErrorDescriptor] = &[INVALID_PARAMS];

impl ProviderErrors for ApprovalProvider {
    fn error_dictionary() -> &'static [ErrorDescriptor] {
        DICTIONARY
    }
}
