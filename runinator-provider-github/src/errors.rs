use runinator_models::errors::{ErrorDescriptor, ProviderErrors};

use crate::GitHubProvider;

pub(crate) use runinator_github::errors::{
    CONFLICTING_AUTHENTICATION, DICTIONARY, HTTP_ERROR, INVALID_PARAMS, MISSING_AUTHENTICATION,
    MISSING_OPERATION_KEY, MISSING_REVIEWERS, REVISION_MISMATCH, UNSUPPORTED_ACTION,
};

impl ProviderErrors for GitHubProvider {
    fn error_dictionary() -> &'static [ErrorDescriptor] {
        DICTIONARY
    }
}
